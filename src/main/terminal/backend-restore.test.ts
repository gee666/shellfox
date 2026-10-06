import { describe, it, expect } from 'vitest';
import { randomUUID } from 'node:crypto';
import { Terminal } from '@xterm/headless';
import type { Result, TerminalAttachmentDto } from '../../shared/contracts';
import { terminalAttachmentSchema } from '../../shared/schemas';
import { PtyBackend, REPLAY_BYTES } from './backend';
import { factoryFixture, launchInput } from './test-fixtures';
import { tuiStream, tuiUpdate } from './tui-fixture';

const value = <T>(r: Result<T>): T => { if (!r.ok) throw new Error(r.error.code); return r.value; };
async function fixture() { const f = factoryFixture(), backend = new PtyBackend(f.options); value(await backend.initialize()); const input = launchInput(); value(await backend.launch(input)); return { ...f, backend, input, pty: f.processes[0]! }; }
const rowsOf = (term: Terminal) => { const b = term.buffer.active; return Array.from({ length: term.rows }, (_, y) => b.getLine(b.viewportY + y)?.translateToString(true) ?? ''); };
async function render(attachment: TerminalAttachmentDto) {
  const term = new Terminal({ cols: attachment.cols, rows: attachment.rows, scrollback: 3000, allowProposedApi: true });
  await new Promise<void>(resolve => term.write(attachment.chunks.map(c => c.data).join(''), resolve)); return term;
}
const VIEW = { cols: 132, rows: 40 };

describe('replay restoration from the screen mirror', () => {
  it('serves a schema-valid serialized screen instead of a partial tail once the ring has evicted output', async () => {
    const f = await fixture(); const id = { tabId: f.input.tabId, generation: f.input.generation };
    value(f.backend.resize({ ...id, ...VIEW }));
    const chunks = tuiStream({ ...VIEW, frames: 4000 }); for (const chunk of chunks) f.pty.output(chunk);
    const reference = new Terminal({ ...VIEW, scrollback: 3000, allowProposedApi: true });
    await new Promise<void>(resolve => reference.write(chunks.join(''), resolve));
    const raw = value(f.backend.attach({ tabId: id.tabId, afterSequence: 1, generation: id.generation }));
    expect(raw).toMatchObject({ truncated: true, snapshot: true, cols: VIEW.cols, rows: VIEW.rows });
    expect(terminalAttachmentSchema.safeParse(raw).success).toBe(true);
    expect(raw.chunks.reduce((n, c) => n + Buffer.byteLength(c.data), 0)).toBeLessThanOrEqual(262144);
    expect(raw.chunks.every((c, i) => c.sequence === raw.firstSequence + i)).toBe(true);
    expect(raw.chunks.at(-1)!.sequence).toBe(raw.lastSequence);
    const restored = await render(raw);
    expect(rowsOf(restored)).toEqual(rowsOf(reference));
    expect(rowsOf(restored).filter(r => /agent \d+ idle/.test(r)).length).toBeGreaterThan(30);
    await f.backend.dispose();
  });
  it('never snapshots when the ring still covers what the view needs', async () => {
    const f = await fixture(); const id = { tabId: f.input.tabId, generation: f.input.generation };
    for (const chunk of tuiStream({ cols: 80, rows: 24, frames: 20 })) f.pty.output(chunk);
    const complete = value(f.backend.attach({ tabId: id.tabId })); expect(complete).toMatchObject({ truncated: false, firstSequence: 1 }); expect(complete.snapshot).toBeUndefined();
    const tail = value(f.backend.attach({ ...id, afterSequence: complete.lastSequence - 2 })); expect(tail.chunks).toHaveLength(2); expect(tail.snapshot).toBeUndefined();
    // A wrong generation with the full history still available is an exact raw replay.
    const other = value(f.backend.attach({ tabId: id.tabId, generation: randomUUID() })); expect(other).toMatchObject({ truncated: true }); expect(other.snapshot).toBeUndefined(); expect(other.chunks).toHaveLength(complete.chunks.length);
    await f.backend.dispose();
  });
  it('keeps the mirror in lockstep with a synchronous burst and with resizes between bursts', async () => {
    const f = await fixture(); const id = { tabId: f.input.tabId, generation: f.input.generation };
    f.pty.output('\x1b[24;1Hold-grid'); value(f.backend.resize({ ...id, cols: 100, rows: 10 })); f.pty.output('\x1b[10;1Hnew-grid');
    for (let i = 0; i < 300; i++) f.pty.output(('\x1b[2;1H' + 'x'.repeat(90)).repeat(45));
    f.pty.output('\x1b[1;1HTOP');
    const a = value(f.backend.attach({ ...id, afterSequence: 0 }));
    expect(a).toMatchObject({ snapshot: true, cols: 100, rows: 10 });
    const term = await render(a); const rows = rowsOf(term);
    expect(rows[0]).toMatch(/^TOP/); expect(rows.some(r => r.includes('new-grid'))).toBe(true); expect(300 * 45 * 98).toBeGreaterThan(REPLAY_BYTES); expect(rows[1]).toMatch(/^x{90}/);
    await f.backend.dispose();
  });
  it('restores the second generation from its own screen after a relaunch', async () => {
    const f = await fixture(); const old = f.input; for (const chunk of tuiStream({ cols: 80, rows: 24, frames: 3000 })) f.pty.output(chunk);
    f.pty.exit(0); const next = { ...old, generation: randomUUID() }; value(await f.backend.launch(next));
    f.processes[1]!.output('\x1b[2J\x1b[Hfresh shell');
    for (let i = 0; i < 300; i++) f.processes[1]!.output('y'.repeat(4096) + '\r\n');
    f.processes[1]!.output('\x1b[H\x1b[2Jprompt$ ');
    const a = value(f.backend.attach({ tabId: next.tabId, generation: old.generation }));
    expect(a).toMatchObject({ generation: next.generation, snapshot: true });
    const rows = rowsOf(await render(a)); expect(rows[0]).toMatch(/^prompt\$/); expect(rows.join('\n')).not.toContain('agent');
    await f.backend.dispose();
  });
  it('re-snapshots a view that fell behind the ring after live data kept flowing (including a 4-byte UTF-16 pair at a chunk boundary)', async () => {
    const f = await fixture(); const id = { tabId: f.input.tabId, generation: f.input.generation };
    for (let i = 0; i < 400; i++) f.pty.output(tuiUpdate({ cols: 80, rows: 24 }, i) + '😀'.repeat(900));
    const a = value(f.backend.attach({ ...id, afterSequence: 3 }));
    expect(a.snapshot).toBe(true); expect(terminalAttachmentSchema.safeParse(a).success).toBe(true);
    for (const c of a.chunks) expect(/[\uD800-\uDBFF]$/.test(c.data)).toBe(false);
    await f.backend.dispose();
  });
});
