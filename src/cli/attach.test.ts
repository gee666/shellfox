import { EventEmitter } from 'node:events';
import { PassThrough, Writable } from 'node:stream';
import { describe, expect, it, vi } from 'vitest';
import { attachTerminal, type AttachApi, type AttachIo } from './attach';
import { failure, success, type Result, type OwnedTerminalAttachmentDto, type TerminalEvent } from '../shared/contracts';
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
function ownedSnapshot(controlEpoch = 1): OwnedTerminalAttachmentDto {
  return { control: 'owned', controlEpoch, tabId, sessionId: tabId, generation, firstSequence: 1, lastSequence: 1,
    chunks: [{ type: 'data', tabId, generation, sequence: 1, data: 'screen' }], truncated: false, snapshot: true,
    state: 'open', exitCode: null, cols: 80, rows: 24, lifetime: 'app-owned' };
}
function fixture(overrides: Partial<AttachApi> = {}) {
  const input = new Input(), output = new Output(), signals = new EventEmitter();
  let listener: (event: TerminalEvent) => void = () => {}, disconnected: () => void = () => {};
  const unsubscribe = vi.fn(), disconnectCleanup = vi.fn();
  const api: AttachApi = {
    attachTerminal: vi.fn<AttachApi['attachTerminal']>(async () => success(ownedSnapshot())),
    takeTerminalControl: vi.fn<AttachApi['takeTerminalControl']>(async () => success({ taken: true })),
    writeTerminal: vi.fn<AttachApi['writeTerminal']>(async () => success({ written: true })),
    resizeTerminal: vi.fn<AttachApi['resizeTerminal']>(async () => success({ resized: true })),
    acknowledgeTerminal: vi.fn<AttachApi['acknowledgeTerminal']>(async () => success({ acknowledged: true })),
    detachTerminal: vi.fn<AttachApi['detachTerminal']>(async () => success({ detached: true })),
    subscribeTerminal: callback => { listener = callback; return unsubscribe; }, ...overrides,
  };
  const io: AttachIo = { input, output, signals, onDisconnect: callback => { disconnected = callback; return disconnectCleanup; } };
  return { input, output, signals, api, io, unsubscribe, disconnectCleanup, emit: (event: TerminalEvent) => listener(event), disconnect: () => disconnected() };
}
async function ready(f: ReturnType<typeof fixture>) {
  await vi.waitFor(() => { expect(f.input.isRaw).toBe(true); expect(f.output.listenerCount('resize')).toBe(1); });
}
describe('interactive attachment', () => {
  it('replays at the host grid, streams output once, ACKs delivery, resizes, and detaches without closing', async () => {
    const f = fixture(), done = attachTerminal(f.api, tabId, f.io); await ready(f);
    expect(f.output.text).toBe(ENTER_TERMINAL_SCREEN + RESET_TERMINAL_MODES + '\u001b[2J\u001b[H\u001b[8;24;80tscreen\u001b[8;40;100t');
    expect(f.api.resizeTerminal).toHaveBeenCalledWith({ tabId, generation, controlEpoch: 1, cols: 100, rows: 40 });
    f.emit({ type: 'data', tabId, generation, controlEpoch: 1, sequence: 1, data: 'duplicate' });
    f.emit({ type: 'data', tabId, generation, controlEpoch: 1, sequence: 2, data: 'live' });
    await vi.waitFor(() => expect(f.api.acknowledgeTerminal).toHaveBeenLastCalledWith({ tabId, generation, controlEpoch: 1, sequence: 2 }));
    f.output.columns = 120; f.output.rows = 50; f.output.emit('resize');
    await vi.waitFor(() => expect(f.api.resizeTerminal).toHaveBeenLastCalledWith({ tabId, generation, controlEpoch: 1, cols: 120, rows: 50 }));
    f.input.emit('data', Buffer.from('typed\u001dnever sent')); await done;
    expect(f.api.writeTerminal).toHaveBeenCalledExactlyOnceWith({ tabId, generation, controlEpoch: 1, data: 'typed' });
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
    f.emit({ type: 'data', tabId, generation, controlEpoch: 1, sequence: 3, data: 'gap' });
    await expect(done).rejects.toThrow('interrupted');
    expect(f.api.acknowledgeTerminal).toHaveBeenCalledExactlyOnceWith({ tabId, generation, controlEpoch: 1, sequence: 1 });
    expect(f.output.text).not.toContain('gap'); expect(f.input.isRaw).toBe(false);
  });
  it('waits for queued output before processing terminal exit', async () => {
    const f = fixture(), done = attachTerminal(f.api, tabId, f.io); await ready(f);
    f.emit({ type: 'data', tabId, generation, controlEpoch: 1, sequence: 2, data: 'last' });
    f.emit({ type: 'exit', tabId, generation, controlEpoch: 1, exitCode: 0, signal: null, lastSequence: 2 });
    await done; expect(f.output.text).toContain('last'); expect(f.input.isRaw).toBe(false);
  });
  it('decodes split UTF-8 input without corrupting characters', async () => {
    const f = fixture(), done = attachTerminal(f.api, tabId, f.io); await ready(f);
    const bytes = Buffer.from('💻'); f.input.emit('data', bytes.subarray(0, 2)); f.input.emit('data', bytes.subarray(2));
    f.input.emit('data', Buffer.from('\u001d')); await done;
    expect(f.api.writeTerminal).toHaveBeenCalledExactlyOnceWith({ tabId, generation, controlEpoch: 1, data: '💻' });
  });
  it('resets remote TUI modes and restores the caller screen and dimensions after detaching', async () => {
    const f = fixture(), done = attachTerminal(f.api, tabId, f.io); await ready(f);
    const modes = '\u001b[?1049h\u001b[?1000h\u001b[?1006h\u001b[?2004h\u001b[?25l\u001b[?1h\u001b[31m\u001b[2;20r';
    f.emit({ type: 'data', tabId, generation, controlEpoch: 1, sequence: 2, data: modes });
    await vi.waitFor(() => expect(f.output.text).toContain(modes));
    f.input.emit('data', Buffer.from('\u001d')); await done;
    expect(f.output.text.startsWith(ENTER_TERMINAL_SCREEN)).toBe(true);
    expect(f.output.text.endsWith(RESET_TERMINAL_MODES + '\u001b[8;40;100t' + LEAVE_TERMINAL_SCREEN)).toBe(true);
    expect(f.output.text.lastIndexOf(RESET_TERMINAL_MODES)).toBeGreaterThan(f.output.text.indexOf(modes));
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
  it('keeps a blocked client blank until explicit take and fresh replay, without resize or input', async () => {
    const f = fixture();
    vi.mocked(f.api.attachTerminal).mockResolvedValueOnce(success({ control: 'blocked', tabId, sessionId: tabId, generation, controlEpoch: 1 }));
    const done = attachTerminal(f.api, tabId, f.io); await ready(f);
    expect(f.output.text).toContain('Another client is controlling this terminal.');
    expect(f.output.text).not.toContain('screen');
    f.input.emit('data', Buffer.from('not sent'));
    f.output.emit('resize');
    f.emit({ type: 'data', tabId, generation, controlEpoch: 1, sequence: 2, data: 'not visible' });
    await Promise.resolve();
    expect(f.api.writeTerminal).not.toHaveBeenCalled(); expect(f.api.resizeTerminal).not.toHaveBeenCalled();
    expect(f.api.acknowledgeTerminal).not.toHaveBeenCalled(); expect(f.output.text).not.toContain('not visible');
    f.input.emit('data', Buffer.from('\x14'));
    await vi.waitFor(() => expect(f.api.takeTerminalControl).toHaveBeenCalledExactlyOnceWith({ tabId, generation }));
    vi.mocked(f.api.attachTerminal).mockResolvedValueOnce(success(ownedSnapshot(2)));
    f.emit({ type: 'control', tabId, generation, control: 'owned', controlEpoch: 2 });
    await vi.waitFor(() => expect(f.api.resizeTerminal).toHaveBeenCalledWith({ tabId, generation, controlEpoch: 2, cols: 100, rows: 40 }));
    expect(f.api.attachTerminal).toHaveBeenCalledTimes(2);
    expect(f.output.text).toContain('screen');
    f.emit({ type: 'control', tabId, generation, control: 'blocked', controlEpoch: 3 });
    await vi.waitFor(() => expect(f.output.text.slice(f.output.text.lastIndexOf('\x1b[2J'))).toContain('Another client'));
    const length = f.output.text.length;
    f.emit({ type: 'data', tabId, generation, controlEpoch: 2, sequence: 2, data: 'old owner data' });
    f.input.emit('data', Buffer.from('also not sent'));
    await Promise.resolve(); expect(f.output.text).toHaveLength(length);
    expect(f.api.writeTerminal).not.toHaveBeenCalled();
    f.input.emit('data', Buffer.from('\x1d')); await done;
    expect(f.api.detachTerminal).toHaveBeenCalledExactlyOnceWith({ tabId, generation });
  });
  it('automatically restores a granted view and fences queued input from its previous ownership', async () => {
    let complete: () => void = () => {};
    const f = fixture({ writeTerminal: vi.fn<AttachApi['writeTerminal']>(() => new Promise(resolve => { complete = () => resolve(success({ written: true })); })) });
    const done = attachTerminal(f.api, tabId, f.io); await ready(f);
    f.input.emit('data', Buffer.from('first'));
    await vi.waitFor(() => expect(f.api.writeTerminal).toHaveBeenCalledOnce());
    f.emit({ type: 'control', tabId, generation, control: 'blocked', controlEpoch: 2 });
    f.input.emit('data', Buffer.from('stale queued input'));
    vi.mocked(f.api.attachTerminal).mockResolvedValueOnce(success(ownedSnapshot(3)));
    f.emit({ type: 'control', tabId, generation, control: 'owned', controlEpoch: 3 });
    complete();
    await vi.waitFor(() => expect(f.api.resizeTerminal).toHaveBeenCalledWith({ tabId, generation, controlEpoch: 3, cols: 100, rows: 40 }));
    expect(f.api.writeTerminal).toHaveBeenCalledExactlyOnceWith({ tabId, generation, controlEpoch: 1, data: 'first' });
    expect(f.api.takeTerminalControl).not.toHaveBeenCalled();
    f.input.emit('data', Buffer.from('\x1d')); await done;
  });
  it('does not paint an owned reply received after a takeover notification', async () => {
    let reply: (value: Awaited<ReturnType<AttachApi['attachTerminal']>>) => void = () => {};
    const attach = vi.fn<AttachApi['attachTerminal']>(() => new Promise(resolve => { reply = resolve; }));
    const f = fixture({ attachTerminal: attach }), done = attachTerminal(f.api, tabId, f.io);
    await vi.waitFor(() => expect(attach).toHaveBeenCalledOnce());
    f.emit({ type: 'control', tabId, generation, control: 'blocked', controlEpoch: 2 });
    const snapshot = ownedSnapshot(1); snapshot.chunks[0].data = 'STALE_PRIVATE_REPLAY';
    reply(success(snapshot)); await ready(f);
    expect(f.output.text).not.toContain('STALE_PRIVATE_REPLAY');
    expect(f.output.text).toContain('Another client is controlling this terminal.');
    expect(f.api.resizeTerminal).not.toHaveBeenCalled(); expect(f.api.acknowledgeTerminal).not.toHaveBeenCalled();
    f.input.emit('data', Buffer.from('\x1d')); await done;
  });
  it('stops a multi-chunk replay when control is lost during an awaited output write', async () => {
    const snapshot = ownedSnapshot(1);
    snapshot.chunks[0].data = 'first permitted chunk';
    snapshot.chunks.push({ type: 'data', tabId, generation, sequence: 2, data: 'STALE_SECOND_CHUNK' });
    snapshot.lastSequence = 2;
    const f = fixture({ attachTerminal: vi.fn(async () => success(snapshot)) });
    vi.spyOn(f.output, '_write').mockImplementation((chunk: Buffer, _encoding, callback) => {
      const data = chunk.toString(); f.output.text += data;
      if (data === 'first permitted chunk') f.emit({ type: 'control', tabId, generation, control: 'blocked', controlEpoch: 2 });
      callback();
    });
    const done = attachTerminal(f.api, tabId, f.io); await ready(f);
    expect(f.output.text).toContain('first permitted chunk');
    expect(f.output.text).not.toContain('STALE_SECOND_CHUNK');
    expect(f.api.resizeTerminal).not.toHaveBeenCalled(); expect(f.api.acknowledgeTerminal).not.toHaveBeenCalled();
    f.input.emit('data', Buffer.from('\x1d')); await done;
  });
  it('forces one initial redraw when the host grid already equals the external grid', async () => {
    const f = fixture(); f.output.columns = 80; f.output.rows = 24;
    const done = attachTerminal(f.api, tabId, f.io); await ready(f);
    expect(vi.mocked(f.api.resizeTerminal).mock.calls.map(([size]) => size)).toEqual([
      { tabId, generation, controlEpoch: 1, cols: 79, rows: 24 },
      { tabId, generation, controlEpoch: 1, cols: 80, rows: 24 },
    ]);
    f.input.emit('data', Buffer.from('\x1d')); await done;
  });
  it('does not forward replay or live device queries to a second terminal responder', async () => {
    const snapshot = ownedSnapshot(); snapshot.chunks[0].data = 'screen\x1b[c\x1b[6n';
    const f = fixture({ attachTerminal: vi.fn(async () => success(snapshot)) });
    vi.spyOn(f.output, '_write').mockImplementation((chunk: Buffer, _encoding, callback) => {
      const data = chunk.toString(); f.output.text += data;
      // Model the external terminal answering DA queries, including replies
      // buffered before interactive input is enabled. Those are not user input.
      if (/\x1b\[(?:>|0)?c/.test(data)) f.input.write('\x1b[?61;6;7;22c');
      callback();
    });
    const done = attachTerminal(f.api, tabId, f.io); await ready(f);
    f.emit({ type: 'data', tabId, generation, controlEpoch: 1, sequence: 2, data: '\x1b[>cLIVE' });
    await vi.waitFor(() => expect(f.api.acknowledgeTerminal).toHaveBeenLastCalledWith({ tabId, generation, controlEpoch: 1, sequence: 2 }));
    expect(f.output.text).toContain('screen'); expect(f.output.text).toContain('LIVE');
    expect(f.output.text).not.toMatch(/\x1b\[(?:>|0)?c/);
    expect(f.api.writeTerminal).not.toHaveBeenCalled();
    f.input.emit('data', Buffer.from('\x1d')); await done;
  });
  it('honors detach during initial replay setup and releases a late attachment', async () => {
    let reply: (value: Awaited<ReturnType<AttachApi['attachTerminal']>>) => void = () => {};
    const attach = vi.fn<AttachApi['attachTerminal']>(() => new Promise(resolve => { reply = resolve; }));
    const f = fixture({ attachTerminal: attach }), done = attachTerminal(f.api, tabId, f.io);
    await vi.waitFor(() => expect(attach).toHaveBeenCalledOnce());
    f.input.emit('data', Buffer.from('\x1d'));
    await done;
    expect(f.input.isRaw).toBe(false);
    reply(success(ownedSnapshot()));
    await vi.waitFor(() => expect(f.api.detachTerminal).toHaveBeenCalledExactlyOnceWith({ tabId, generation }));
    expect(f.api.resizeTerminal).not.toHaveBeenCalled(); expect(f.output.text).not.toContain('screen');
  });
  it('stops waiting for a resize reply when detaching without pending input', async () => {
    let resizeReply: (value: Awaited<ReturnType<AttachApi['resizeTerminal']>>) => void = () => {};
    const resize = vi.fn<AttachApi['resizeTerminal']>(() => new Promise(resolve => { resizeReply = resolve; }));
    const f = fixture({ resizeTerminal: resize }), done = attachTerminal(f.api, tabId, f.io);
    await vi.waitFor(() => expect(resize).toHaveBeenCalledOnce());
    f.input.emit('data', Buffer.from('\x1d')); await done;
    expect(f.api.detachTerminal).toHaveBeenCalledOnce(); expect(f.input.isRaw).toBe(false);
    resizeReply(success({ resized: true }));
  });
  it.each(['resize', 'ack'] as const)('ignores a late %s ownership rejection after restoring the caller screen', async method => {
    let rejectRequest: () => void = () => {};
    const delayed = new Promise<Result<never>>(resolve => { rejectRequest = () => resolve(failure('TERMINAL_CONTROL_REQUIRED', 'Taken by another client.')); });
    const f = fixture(method === 'resize' ? { resizeTerminal: vi.fn(() => delayed) } : { acknowledgeTerminal: vi.fn(() => delayed) });
    const done = attachTerminal(f.api, tabId, f.io);
    await vi.waitFor(() => expect(method === 'resize' ? f.api.resizeTerminal : f.api.acknowledgeTerminal).toHaveBeenCalledOnce());
    f.input.emit('data', Buffer.from('\x1d')); await done;
    expect(f.output.text.endsWith(LEAVE_TERMINAL_SCREEN)).toBe(true);
    const restored = f.output.text;
    rejectRequest(); await delayed;
    await new Promise<void>(resolve => setImmediate(resolve));
    expect(f.output.text).toBe(restored);
  });
  it('does not adopt temporary replay grid changes as the external viewport size', async () => {
    const f = fixture();
    vi.mocked(f.api.attachTerminal).mockResolvedValueOnce(success({ control: 'blocked', tabId, sessionId: tabId, generation, controlEpoch: 1 }));
    const done = attachTerminal(f.api, tabId, f.io); await ready(f);
    vi.spyOn(f.output, '_write').mockImplementation((chunk: Buffer, _encoding, callback) => {
      const data = chunk.toString(); f.output.text += data;
      const size = /\x1b\[8;(\d+);(\d+)t/.exec(data);
      if (size) { f.output.rows = Number(size[1]); f.output.columns = Number(size[2]); f.output.emit('resize'); }
      callback();
    });
    vi.mocked(f.api.attachTerminal).mockResolvedValueOnce(success(ownedSnapshot(2)));
    f.emit({ type: 'control', tabId, generation, control: 'owned', controlEpoch: 2 });
    await vi.waitFor(() => expect(f.api.resizeTerminal).toHaveBeenCalledOnce());
    expect(f.api.resizeTerminal).toHaveBeenCalledWith({ tabId, generation, controlEpoch: 2, cols: 100, rows: 40 });
    expect(f.output.columns).toBe(100); expect(f.output.rows).toBe(40);
    f.input.emit('data', Buffer.from('\x1d')); await done;
  });
  it('coalesces 1500 resize events while a host reply is delayed', async () => {
    const f = fixture(), done = attachTerminal(f.api, tabId, f.io); await ready(f);
    let complete: () => void = () => {};
    vi.mocked(f.api.resizeTerminal).mockImplementationOnce(() => new Promise(resolve => { complete = () => resolve(success({ resized: true })); }));
    f.output.columns = 101; f.output.emit('resize');
    await vi.waitFor(() => expect(f.api.resizeTerminal).toHaveBeenCalledTimes(2));
    for (let i = 0; i < 1500; i++) { f.output.columns = 102 + i % 200; f.output.emit('resize'); }
    expect(f.api.resizeTerminal).toHaveBeenCalledTimes(2);
    complete();
    await vi.waitFor(() => expect(f.api.resizeTerminal).toHaveBeenCalledTimes(3));
    expect(f.api.resizeTerminal).toHaveBeenLastCalledWith({ tabId, generation, controlEpoch: 1, cols: 201, rows: 40 });
    f.input.emit('data', Buffer.from('\x1d')); await done;
  });
  it('rejects non-TTY input before attaching', async () => {
    const f = fixture(); f.input.isTTY = false;
    await expect(attachTerminal(f.api, tabId, f.io)).rejects.toThrow('requires a TTY');
    expect(f.api.attachTerminal).not.toHaveBeenCalled();
  });
});
