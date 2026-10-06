import { describe, it, expect, vi } from 'vitest';
import { randomUUID } from 'node:crypto';
import type { Result, TerminalEvent } from '../../shared/contracts';
import { terminalAttachmentSchema, terminalEventSchema } from '../../shared/schemas';
import { PtyBackend, REPLAY_BYTES } from './backend';
import { factoryFixture, launchInput } from './test-fixtures';
const value = <T>(r: Result<T>): T => { if (!r.ok) throw new Error(r.error.code); return r.value; };
async function fixture() { const f = factoryFixture(), backend = new PtyBackend(f.options); value(await backend.initialize()); const input = launchInput(); value(await backend.launch(input)); return { ...f, backend, input, pty: f.processes[0] }; }
describe('owned PTY backend', () => {
  it('caps outstanding native input across arbitrarily many rate windows', async () => {
    const f = await fixture(); f.pty.queuedInputBytes.mockReturnValue(128 * 1024);
    for (let i = 0; i < 100; i++) expect(f.backend.write({ tabId: f.input.tabId, generation: f.input.generation, data: 'x' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    expect(f.pty.write).not.toHaveBeenCalled(); f.pty.queuedInputBytes.mockReturnValue(null);
    expect(f.backend.write({ tabId: f.input.tabId, generation: f.input.generation, data: 'x' })).toMatchObject({ ok: false, error: { code: 'NATIVE_UNAVAILABLE' } });
    f.pty.queuedInputBytes.mockReturnValue(0); value(f.backend.write({ tabId: f.input.tabId, generation: f.input.generation, data: 'x' })); await f.backend.dispose();
  });
  it('never uses node-pty numeric Unix kill, including a delayed exit callback', async () => {
    const f = factoryFixture(); const terminateUnix = vi.fn(async () => { f.processes[0].exit(0); });
    const backend = new PtyBackend({ ...f.options, platform: 'linux', terminateUnix }); value(await backend.initialize()); const input = launchInput(); value(await backend.launch(input));
    f.processes[0].kill.mockImplementation(() => { throw new Error('unsafe numeric kill'); });
    value(await backend.close({ tabId: input.tabId, generation: input.generation })); expect(terminateUnix).toHaveBeenCalledTimes(1); expect(f.processes[0].kill).not.toHaveBeenCalled(); await backend.dispose();
  });
  it('retains failed handles and refuses successful shutdown until exact exit is confirmed', async () => {
    const f = await fixture(); f.pty.kill.mockImplementation(() => { throw new Error('denied'); });
    expect(await f.backend.close({ tabId: f.input.tabId, generation: f.input.generation })).toMatchObject({ ok: false, error: { code: 'NATIVE_UNAVAILABLE' } });
    await expect(f.backend.dispose()).rejects.toThrow(); expect(f.backend.live()).toHaveLength(1); expect(f.backend.get(f.input.tabId)?.state).toBe('open');
    f.pty.kill.mockImplementation(() => f.pty.exit(0)); await f.backend.dispose(); expect(f.backend.live()).toHaveLength(0);
  });
  it('does not treat a kill call without an exit event as confirmed closure', async () => {
    const f = factoryFixture(), backend = new PtyBackend({ ...f.options, closeTimeoutMs: 5 }); value(await backend.initialize()); const input = launchInput(); value(await backend.launch(input)); f.processes[0].kill.mockImplementation(() => {});
    expect(await backend.close({ tabId: input.tabId, generation: input.generation })).toMatchObject({ ok: false, error: { code: 'NATIVE_UNAVAILABLE' } }); expect(backend.get(input.tabId)?.state).toBe('open');
    f.processes[0].exit(0); await backend.dispose();
  });
  it('refuses to terminate a WSL transport until guest termination is verified', async () => {
    let denied = true; const f = factoryFixture(), backend = new PtyBackend({ ...f.options, terminateGuest: async () => { if (denied) throw new Error('unknown guest root'); } }); value(await backend.initialize()); const input = { ...launchInput(), profileId: 'wsl:Ubuntu' }; value(await backend.launch(input));
    expect(await backend.close({ tabId: input.tabId, generation: input.generation })).toMatchObject({ ok: false, error: { code: 'MONITOR_UNAVAILABLE' } }); expect(f.processes[0].kill).not.toHaveBeenCalled(); expect(backend.live()).toHaveLength(1); denied = false; f.processes[0].exit(0); await backend.dispose();
  });
  it('spawns an interactive login shell in the validated cwd with terminal env and a unique marker', async () => {
    const f = await fixture();
    expect(f.factory).toHaveBeenCalledWith('/bin/bash', ['-l', '-i'], expect.objectContaining({ cwd: '/work', cols: 80, rows: 24, name: 'xterm-256color', env: expect.objectContaining({ TERM: 'xterm-256color', SHELLFOX_TERMINAL_MARKER: expect.any(String) }) }));
    expect(f.backend.live()).toHaveLength(1); await f.backend.dispose();
  });
  it.each(['win32', 'linux'] as const)('preserves explicit terminal capabilities in spawn env and node-pty name on %s', async platform => {
    const f = factoryFixture(), testBackend = new PtyBackend({ ...f.options, platform });
    value(await testBackend.initialize());
    const input = { ...launchInput(), env: [{ name: 'TERM', value: 'vt100' }, { name: 'COLORTERM', value: '' }] };
    value(await testBackend.launch(input));
    expect(f.factory).toHaveBeenLastCalledWith(expect.any(String), expect.any(Array), expect.objectContaining({ name: 'vt100', env: expect.objectContaining({ TERM: 'vt100', COLORTERM: '' }), useConpty: true, useConptyDll: true }));
    await testBackend.dispose();
  });
  it('restores an explicit empty TERM after Unix node-pty chooses its fallback name', async () => {
    const f = factoryFixture(), backend = new PtyBackend({ ...f.options, platform: 'linux' }); value(await backend.initialize());
    value(await backend.launch({ ...launchInput(), env: [{ name: 'TERM', value: '' }] }));
    expect(f.factory).toHaveBeenLastCalledWith('/usr/bin/env', ['TERM=', '/bin/bash', '-l', '-i'], expect.objectContaining({ name: '', env: expect.objectContaining({ TERM: '', COLORTERM: 'truecolor' }) }));
    await backend.dispose();
  });
  it('launches WSL with guest cwd and injected marker, never an interpolated shell command', async () => {
    const f = await fixture(), input = { ...launchInput(), profileId: 'wsl:Ubuntu', cwd: "/home/user/a';& folder" };
    value(await f.backend.launch(input));
    expect(f.factory).toHaveBeenLastCalledWith('C:\\Windows\\System32\\wsl.exe', ['--distribution', 'Ubuntu', '--cd', input.cwd, '--exec', '/usr/bin/env', expect.stringMatching(/^SHELLFOX_TERMINAL_MARKER=/), '/bin/bash', '-l', '-i'], expect.anything());
    expect(f.backend.get(input.tabId)?.root).toMatchObject({ environment: 'wsl', distro: 'Ubuntu' }); await f.backend.dispose();
  });
  it('deduplicates a generation and forbids replacing a live tab', async () => {
    const f = await fixture(); value(await f.backend.launch(f.input)); expect(f.factory).toHaveBeenCalledTimes(1);
    expect(await f.backend.launch({ ...f.input, generation: randomUUID() })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } }); await f.backend.dispose();
  });
  it('never invokes a factory for unavailable profiles or inaccessible folders', async () => {
    const f = factoryFixture(), b = new PtyBackend({ ...f.options, validateCwd: async () => { throw new Error('inaccessible'); } }); value(await b.initialize());
    expect(await b.launch({ ...launchInput(), profileId: 'arbitrary-exe' })).toMatchObject({ ok: false, error: { code: 'DEPENDENCY_MISSING' } });
    expect(await b.launch(launchInput())).toMatchObject({ ok: false, error: { code: 'VALIDATION' } }); expect(f.factory).not.toHaveBeenCalled(); await b.dispose();
  });
  it('returns honest addon/discovery failures', async () => {
    const b = new PtyBackend({ discover: async () => { throw new Error('not installed'); }, factory: vi.fn() });
    expect(await b.initialize()).toMatchObject({ ok: false, error: { code: 'DEPENDENCY_MISSING' } }); await b.dispose();
  });
  it('keeps raw VT controls and Unicode intact through ordered bounded chunks', async () => {
    const f = await fixture(), events: TerminalEvent[] = [], stop = f.backend.subscribe(e => events.push(e));
    const output = '\x1b[31m' + '雪🙂'.repeat(5000) + '\x00\r\n'; f.pty.output(output);
    const attached = value(f.backend.attach({ tabId: f.input.tabId }));
    expect(attached.chunks.map(c => c.data).join('')).toBe(output); expect(attached.chunks.every(c => Buffer.byteLength(c.data) <= 16384)).toBe(true);
    expect(attached.chunks.map(c => c.sequence)).toEqual(attached.chunks.map((_, i) => i + 1));
    expect(events.every(e => terminalEventSchema.safeParse(e).success)).toBe(true); expect(terminalAttachmentSchema.safeParse(attached).success).toBe(true); stop(); await f.backend.dispose();
  });
  it('discards old output by bytes and replays only after the cursor', async () => {
    const f = await fixture(); f.pty.output('x'.repeat(REPLAY_BYTES * 3));
    const all = value(f.backend.attach({ tabId: f.input.tabId }));
    expect(all.truncated).toBe(true); expect(all.chunks.reduce((n, c) => n + Buffer.byteLength(c.data), 0)).toBeLessThanOrEqual(REPLAY_BYTES);
    const tail = value(f.backend.attach({ tabId: f.input.tabId, afterSequence: all.lastSequence - 1, generation: f.input.generation })); expect(tail.chunks).toHaveLength(1); expect(tail.truncated).toBe(false);
    expect(value(f.backend.attach({ tabId: f.input.tabId, generation: randomUUID() })).truncated).toBe(true);
    expect(f.backend.attach({ tabId: f.input.tabId, afterSequence: all.lastSequence + 1 })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } }); await f.backend.dispose();
  });
  it('bounds tiny-chunk replay count as well as bytes', async () => {
    const f = await fixture(); for (let i = 0; i < 9000; i++) f.pty.output('x');
    const a = value(f.backend.attach({ tabId: f.input.tabId })); expect(a.chunks.length).toBeLessThanOrEqual(8192); expect(a.truncated).toBe(true); await f.backend.dispose();
  });
  it('reattach and unsubscribe do not spawn or close shells', async () => {
    const f = await fixture(); f.backend.attach({ tabId: f.input.tabId }); f.backend.attach({ tabId: f.input.tabId }); f.backend.subscribe(() => {})();
    expect(f.factory).toHaveBeenCalledTimes(1); expect(f.pty.kill).not.toHaveBeenCalled(); await f.backend.dispose();
  });
  it('validates generation, input bytes, NUL, dimension bounds and input rate', async () => {
    const f = await fixture(), identity = { tabId: f.input.tabId, generation: f.input.generation };
    expect(f.backend.write({ ...identity, generation: randomUUID(), data: 'x' })).toMatchObject({ ok: false, error: { code: 'TARGET_LOST' } });
    for (const data of ['', '\0', '雪'.repeat(30000)]) expect(f.backend.write({ ...identity, data }).ok).toBe(false);
    for (const cols of [1, 501, 2.5]) expect(f.backend.resize({ ...identity, cols, rows: 24 }).ok).toBe(false);
    value(f.backend.resize({ ...identity, cols: 100, rows: 40 })); expect(f.pty.resize).toHaveBeenCalledWith(100, 40);
    for (let i = 0; i < 4; i++) value(f.backend.write({ ...identity, data: 'x'.repeat(65536) }));
    expect(f.backend.write({ ...identity, data: 'x' }).ok).toBe(false); expect(f.pty.write).toHaveBeenCalledTimes(4); await f.backend.dispose();
  });
  it('sends one authoritative exit, keeps replay, and closes only exact owned handles', async () => {
    const f = await fixture(), events: TerminalEvent[] = []; f.backend.subscribe(e => events.push(e)); f.pty.output('done'); f.pty.exit(7); f.pty.exit(8);
    const a = value(f.backend.attach({ tabId: f.input.tabId })); expect(a).toMatchObject({ state: 'closed', exitCode: 7 }); expect(events.filter(e => e.type === 'exit')).toHaveLength(1);
    expect((await f.backend.close({ tabId: randomUUID(), generation: f.input.generation })).ok).toBe(false);
    expect(f.backend.write({ tabId: f.input.tabId, generation: f.input.generation, data: 'x' }).ok).toBe(false);
    value(await f.backend.close({ tabId: f.input.tabId, generation: f.input.generation })); expect(f.pty.kill).not.toHaveBeenCalled(); await f.backend.dispose();
  });
  it('ignores events from an old closed generation after replacement', async () => {
    const f = await fixture(); f.pty.exit(0); const next = { ...f.input, generation: randomUUID() }; value(await f.backend.launch(next)); f.pty.output('stale'); f.pty.exit(99);
    expect(value(f.backend.attach({ tabId: next.tabId })).generation).toBe(next.generation); expect(f.backend.get(next.tabId)?.state).toBe('open'); await f.backend.dispose();
  });
  it('disposal terminates live owned shells without touching process IDs directly', async () => {
    const f = await fixture(); await f.backend.dispose(); await f.backend.dispose(); expect(f.pty.kill).toHaveBeenCalledTimes(1); expect(f.backend.live()).toHaveLength(0);
    expect(await f.backend.launch(launchInput())).toMatchObject({ ok: false, error: { code: 'NATIVE_UNAVAILABLE' } });
  });
});
