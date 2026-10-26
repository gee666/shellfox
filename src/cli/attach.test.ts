import { EventEmitter } from 'node:events';
import { PassThrough, Writable } from 'node:stream';
import { describe, expect, it, vi } from 'vitest';
import { attachTerminal, type AttachApi, type AttachIo } from './attach';
import { success, type TerminalEvent } from '../shared/contracts';
import { ENTER_TERMINAL_SCREEN, LEAVE_TERMINAL_SCREEN, RESET_TERMINAL_MODES } from './terminal-presentation';
const tabId = '11111111-1111-4111-8111-111111111111', generation = '22222222-2222-4222-8222-222222222222';
class Input extends PassThrough {
  isTTY = true; isRaw = false;
  setRawMode(value: boolean) { this.isRaw = value; return this; }
}
class Output extends Writable {
  isTTY = true; columns = 100; rows = 40; text = '';
  override _write(chunk: Buffer, _encoding: BufferEncoding, callback: (error?: Error | null) => void) { this.text += chunk.toString(); callback(); }
}
function fixture(overrides: Partial<AttachApi> = {}) {
  const input = new Input(), output = new Output(), signals = new EventEmitter();
  let listener: (event: TerminalEvent) => void = () => {}, disconnected: () => void = () => {};
  const unsubscribe = vi.fn(), disconnectCleanup = vi.fn();
  const api: AttachApi = {
    attachTerminal: vi.fn<AttachApi['attachTerminal']>(async () => success({ tabId, sessionId: tabId, generation, firstSequence: 1, lastSequence: 1,
      chunks: [{ type: 'data', tabId, generation, sequence: 1, data: 'screen' }], truncated: false, snapshot: true, state: 'open', exitCode: null, cols: 80, rows: 24, lifetime: 'app-owned' })),
    writeTerminal: vi.fn<AttachApi['writeTerminal']>(async () => success({ written: true })),
    resizeTerminal: vi.fn<AttachApi['resizeTerminal']>(async () => success({ resized: true })),
    acknowledgeTerminal: vi.fn<AttachApi['acknowledgeTerminal']>(async () => success({ acknowledged: true })),
    detachTerminal: vi.fn<AttachApi['detachTerminal']>(async () => success({ detached: true })),
    subscribeTerminal: callback => { listener = callback; return unsubscribe; }, ...overrides,
  };
  const io: AttachIo = { input, output, signals, onDisconnect: callback => { disconnected = callback; return disconnectCleanup; } };
  return { input, output, signals, api, io, unsubscribe, disconnectCleanup, emit: (event: TerminalEvent) => listener(event), disconnect: () => disconnected() };
}
async function ready(f: ReturnType<typeof fixture>) { await vi.waitFor(() => expect(f.input.isRaw).toBe(true)); }
describe('interactive attachment', () => {
  it('replays at the host grid, streams output once, ACKs delivery, resizes, and detaches without closing', async () => {
    const f = fixture(), done = attachTerminal(f.api, tabId, f.io); await ready(f);
    expect(f.output.text).toBe(ENTER_TERMINAL_SCREEN + '\u001b[8;24;80tscreen\u001b[8;40;100t');
    expect(f.api.resizeTerminal).toHaveBeenCalledWith({ tabId, generation, cols: 100, rows: 40 });
    f.emit({ type: 'data', tabId, generation, sequence: 1, data: 'duplicate' });
    f.emit({ type: 'data', tabId, generation, sequence: 2, data: 'live' });
    await vi.waitFor(() => expect(f.api.acknowledgeTerminal).toHaveBeenLastCalledWith({ tabId, generation, sequence: 2 }));
    f.output.columns = 120; f.output.rows = 50; f.output.emit('resize');
    await vi.waitFor(() => expect(f.api.resizeTerminal).toHaveBeenLastCalledWith({ tabId, generation, cols: 120, rows: 50 }));
    f.input.emit('data', Buffer.from('typed\u001dnever sent')); await done;
    expect(f.api.writeTerminal).toHaveBeenCalledExactlyOnceWith({ tabId, generation, data: 'typed' });
    expect(f.output.text).not.toContain('duplicate'); expect(f.output.text).toContain('live');
    expect(f.api.detachTerminal).toHaveBeenCalledExactlyOnceWith({ tabId, generation });
    expect(f.input.isRaw).toBe(false); expect(f.unsubscribe).toHaveBeenCalledOnce(); expect(f.disconnectCleanup).toHaveBeenCalledOnce();
    expect(f.input.listenerCount('data')).toBe(0); expect(f.output.listenerCount('resize')).toBe(0); expect(f.signals.eventNames()).toEqual([]);
  });
  it.each(['end', 'SIGINT', 'SIGTERM'])('restores raw mode and detaches on %s', async event => {
    const f = fixture(), done = attachTerminal(f.api, tabId, f.io); await ready(f);
    if (event === 'end') f.input.emit('end'); else f.signals.emit(event);
    await done; expect(f.input.isRaw).toBe(false); expect(f.api.detachTerminal).toHaveBeenCalledOnce();
  });
  it('restores the original raw state rather than always disabling it', async () => {
    const f = fixture(); f.input.isRaw = true;
    const done = attachTerminal(f.api, tabId, f.io);
    await vi.waitFor(() => expect(f.api.resizeTerminal).toHaveBeenCalledOnce());
    f.signals.emit('SIGTERM'); await done; expect(f.input.isRaw).toBe(true);
  });
  it('restores on transport loss without retrying the connection or stopping the host', async () => {
    const f = fixture(), done = attachTerminal(f.api, tabId, f.io); await ready(f);
    f.disconnect(); await expect(done).rejects.toThrow('Input delivery may be uncertain');
    expect(f.input.isRaw).toBe(false); expect(f.api.attachTerminal).toHaveBeenCalledOnce();
  });
  it('never retries an uncertain write or sends later queued input', async () => {
    const f = fixture({ writeTerminal: vi.fn(async () => { throw new Error('lost reply'); }) });
    const done = attachTerminal(f.api, tabId, f.io); await ready(f);
    f.input.emit('data', Buffer.from('first')); f.input.emit('data', Buffer.from('second'));
    await expect(done).rejects.toThrow('lost reply');
    expect(f.api.writeTerminal).toHaveBeenCalledOnce(); expect(f.input.isRaw).toBe(false);
  });
  it('rejects missing output rather than ACKing an unseen sequence', async () => {
    const f = fixture(), done = attachTerminal(f.api, tabId, f.io); await ready(f);
    f.emit({ type: 'data', tabId, generation, sequence: 3, data: 'gap' });
    await expect(done).rejects.toThrow('interrupted');
    expect(f.api.acknowledgeTerminal).toHaveBeenCalledExactlyOnceWith({ tabId, generation, sequence: 1 });
    expect(f.output.text).not.toContain('gap'); expect(f.input.isRaw).toBe(false);
  });
  it('waits for queued output before processing terminal exit', async () => {
    const f = fixture(), done = attachTerminal(f.api, tabId, f.io); await ready(f);
    f.emit({ type: 'data', tabId, generation, sequence: 2, data: 'last' });
    f.emit({ type: 'exit', tabId, generation, exitCode: 0, signal: null, lastSequence: 2 });
    await done; expect(f.output.text).toContain('last'); expect(f.input.isRaw).toBe(false);
  });
  it('decodes split UTF-8 input without corrupting characters', async () => {
    const f = fixture(), done = attachTerminal(f.api, tabId, f.io); await ready(f);
    const bytes = Buffer.from('💻'); f.input.emit('data', bytes.subarray(0, 2)); f.input.emit('data', bytes.subarray(2));
    f.input.emit('data', Buffer.from('\u001d')); await done;
    expect(f.api.writeTerminal).toHaveBeenCalledExactlyOnceWith({ tabId, generation, data: '💻' });
  });
  it('resets remote TUI modes and restores the caller screen and dimensions after detaching', async () => {
    const f = fixture(), done = attachTerminal(f.api, tabId, f.io); await ready(f);
    const modes = '\u001b[?1049h\u001b[?1000h\u001b[?1006h\u001b[?2004h\u001b[?25l\u001b[?1h\u001b[31m\u001b[2;20r';
    f.emit({ type: 'data', tabId, generation, sequence: 2, data: modes });
    await vi.waitFor(() => expect(f.output.text).toContain(modes));
    f.input.emit('data', Buffer.from('\u001d')); await done;
    expect(f.output.text.startsWith(ENTER_TERMINAL_SCREEN)).toBe(true);
    expect(f.output.text.endsWith(RESET_TERMINAL_MODES + '\u001b[8;40;100t' + LEAVE_TERMINAL_SCREEN)).toBe(true);
    expect(f.output.text.indexOf(RESET_TERMINAL_MODES)).toBeGreaterThan(f.output.text.indexOf(modes));
  });
  it('restores presentation even when replay fails before the initial grid is restored', async () => {
    const f = fixture({ acknowledgeTerminal: vi.fn(async () => { throw new Error('replay ACK failed'); }) });
    await expect(attachTerminal(f.api, tabId, f.io)).rejects.toThrow('replay ACK failed');
    expect(f.output.text).toContain('\u001b[8;24;80tscreen');
    expect(f.output.text.endsWith(RESET_TERMINAL_MODES + '\u001b[8;40;100t' + LEAVE_TERMINAL_SCREEN)).toBe(true);
    expect(f.input.isRaw).toBe(false);
  });
  it('bounds pending input while an earlier write reply is delayed, with no retries', async () => {
    let complete: () => void = () => {};
    const writeTerminal = vi.fn<AttachApi['writeTerminal']>(() => new Promise(resolve => { complete = () => resolve(success({ written: true })); }));
    const f = fixture({ writeTerminal }), done = attachTerminal(f.api, tabId, f.io); await ready(f);
    f.input.emit('data', Buffer.alloc(65536, 'a'));
    await vi.waitFor(() => expect(writeTerminal).toHaveBeenCalledOnce());
    f.input.emit('data', Buffer.alloc(65536, 'b')); f.input.emit('data', Buffer.from('over limit'));
    await expect(done).rejects.toThrow('128 KiB pending-input limit');
    complete(); await Promise.resolve();
    expect(writeTerminal).toHaveBeenCalledOnce(); expect(f.input.isRaw).toBe(false);
  });
  it('rejects oversized input frames explicitly instead of truncating paste', async () => {
    const f = fixture(), done = attachTerminal(f.api, tabId, f.io); await ready(f);
    f.input.emit('data', Buffer.alloc(65537, 'x'));
    await expect(done).rejects.toThrow('64 KiB frame');
    expect(f.api.writeTerminal).not.toHaveBeenCalled(); expect(f.input.isRaw).toBe(false);
  });
  it.each(['input', 'output'] as const)('restores presentation and raw mode after %s errors', async target => {
    const f = fixture(), done = attachTerminal(f.api, tabId, f.io); await ready(f);
    f[target].emit('error', new Error('terminal disappeared'));
    await expect(done).rejects.toThrow('terminal disappeared');
    expect(f.input.isRaw).toBe(false); expect(f.output.text.endsWith(LEAVE_TERMINAL_SCREEN)).toBe(true);
  });
  it('rejects non-TTY input before attaching', async () => {
    const f = fixture(); f.input.isTTY = false;
    await expect(attachTerminal(f.api, tabId, f.io)).rejects.toThrow('requires a TTY');
    expect(f.api.attachTerminal).not.toHaveBeenCalled();
  });
});
