import { describe, it, expect } from 'vitest';
import { Terminal } from '@xterm/headless';
import { SerializeAddon } from '@xterm/addon-serialize';
import { ScreenMirror } from './screen-mirror';
import { tuiStream } from './tui-fixture';

const settle = () => new Promise<void>(resolve => setTimeout(resolve, 0));
async function drained(mirror: ScreenMirror, sequence: number) { for (let i = 0; i < 500 && mirror.sequence < sequence; i++) await settle(); }
async function parse(term: Terminal, data: string) { await new Promise<void>(resolve => term.write(data, resolve)); }
function screen(term: Terminal) {
  const buffer = term.buffer.active; const rows: string[] = [];
  for (let y = 0; y < term.rows; y++) rows.push(buffer.getLine(buffer.viewportY + y)?.translateToString(true) ?? '');
  return rows;
}
function state(term: Terminal) {
  const serializer = new SerializeAddon(); term.loadAddon(serializer as never);
  return { screen: screen(term), alt: term.buffer.active.type, cursor: [term.buffer.active.cursorX, term.buffer.active.cursorY], serialized: serializer.serialize() };
}
async function restoreFrom(cols: number, rows: number, data: string) { const term = new Terminal({ cols, rows, scrollback: 3000, allowProposedApi: true }); await parse(term, data); return term; }

describe('ScreenMirror', () => {
  for (const alt of [false, true]) it(`restores a diff-rendered TUI that a byte tail cannot (${alt ? 'alternate' : 'normal'} buffer)`, async () => {
    const options = { cols: 120, rows: 30, alt, frames: 6000 };
    const chunks = tuiStream(options);
    const mirror = new ScreenMirror(options.cols, options.rows);
    const reference = new Terminal({ cols: options.cols, rows: options.rows, scrollback: 3000, allowProposedApi: true });
    chunks.forEach((chunk, i) => mirror.write(i + 1, chunk));
    await parse(reference, chunks.join(''));
    await drained(mirror, chunks.length);
    expect(mirror.sequence).toBe(chunks.length);

    // What the old replay produced: only the last 256 KiB of output into a fresh terminal.
    let tail = '', size = 0;
    for (let i = chunks.length - 1; i >= 0 && size + chunks[i].length <= 256 * 1024; i--) { tail = chunks[i] + tail; size += chunks[i].length; }
    const broken = await restoreFrom(options.cols, options.rows, tail);
    expect(screen(broken).filter(line => line.includes('agent'))).toHaveLength(0);

    const snapshot = mirror.snapshot(200 * 1024)!;
    expect(snapshot).toMatchObject({ sequence: chunks.length, cols: options.cols, rows: options.rows });
    const restored = await restoreFrom(snapshot.cols, snapshot.rows, snapshot.data);
    const expected = state(reference), actual = state(restored);
    expect(actual.screen).toEqual(expected.screen);
    expect(actual.screen.filter(line => line.includes('agent'))).toHaveLength(options.rows - 2);
    expect(actual.screen[0]).toContain('pi  coding agent');
    expect(actual.alt).toBe(expected.alt);
    expect(actual.cursor).toEqual(expected.cursor);
    expect(actual.serialized).toBe(expected.serialized);
    // Live output after restore keeps matching the reference.
    const more = '\x1b[5;41H\x1b[1mNEW\x1b[0m';
    await parse(reference, more); await parse(restored, more);
    expect(screen(restored)).toEqual(screen(reference));
    mirror.dispose();
  });
  it('restores what the serializer alone omits: scroll region, cursor visibility, mouse encoding, pending-wrap cursor, modes', async () => {
    const mirror = new ScreenMirror(20, 6);
    mirror.write(1, '\x1b[?1049h\x1b[?25l\x1b[?1h\x1b[?1000h\x1b[?1006h\x1b[?2004h\x1b[2;5r\x1b[6;1H' + 'w'.repeat(20));
    const { data } = mirror.snapshot(100000)!;
    const restored = new Terminal({ cols: 20, rows: 6, allowProposedApi: true });
    await new Promise<void>(resolve => restored.write(data, resolve));
    const core = (restored as unknown as { _core: { buffer: { scrollTop: number; scrollBottom: number }; coreMouseService: { activeProtocol: string; activeEncoding: string }; coreService: { isCursorHidden: boolean } } })._core;
    expect(restored.buffer.active.type).toBe('alternate');
    expect([core.buffer.scrollTop, core.buffer.scrollBottom]).toEqual([1, 4]);
    expect(core.coreService.isCursorHidden).toBe(true);
    expect(core.coreMouseService).toMatchObject({ activeProtocol: 'VT200', activeEncoding: 'SGR' });
    expect(restored.modes).toMatchObject({ applicationCursorKeysMode: true, bracketedPasteMode: true });
    expect([restored.buffer.active.cursorX, restored.buffer.active.cursorY]).toEqual([19, 5]);
    mirror.dispose();
  });
  it('keeps resizes ordered with output', async () => {
    const mirror = new ScreenMirror(80, 24);
    mirror.write(1, '\x1b[24;1Hbottom-old'); mirror.resize(100, 10); mirror.write(2, '\x1b[10;1Hbottom-new');
    await settle();
    const snapshot = mirror.snapshot(100000)!;
    expect(snapshot).toMatchObject({ sequence: 2, cols: 100, rows: 10 });
    expect(snapshot.data).toContain('bottom-new');
    mirror.dispose();
  });
  it('shortens history before giving up on the screen, and can refuse', async () => {
    const mirror = new ScreenMirror(80, 24);
    mirror.write(1, Array.from({ length: 2000 }, (_, i) => `line ${i} ${'x'.repeat(60)}`).join('\r\n') + '\r\nlast');
    await settle();
    const small = mirror.snapshot(40 * 1024)!;
    expect(Buffer.byteLength(small.data)).toBeLessThanOrEqual(40 * 1024); expect(small.data).toContain('last');
    expect(mirror.snapshot(10)).toBeNull();
    mirror.dispose(); expect(mirror.snapshot(1000)).toBeNull();
  });
});
