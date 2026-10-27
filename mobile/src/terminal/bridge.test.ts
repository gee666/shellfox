import { afterEach, describe, expect, it, vi } from 'vitest';
import { failure, success, type TerminalAttachmentDto, type OwnedTerminalAttachmentDto, type TerminalEvent } from '../../../src/shared/contracts';
import { TerminalBridge, type MobileTerminalApi } from './bridge';
import { emptyModifiers, type HostMessage } from './protocol';
import { TerminalOperationError } from './errors';
const tabId = '12345678-1234-4234-8234-123456789abc', generation = '12345678-1234-4234-8234-123456789abd';
const event = (sequence: number, data = `${sequence}`): TerminalEvent & { type: 'data' } => ({ type: 'data', tabId, generation, controlEpoch: 1, sequence, data });
const snapshot = (chunks = [event(1, 'screen')], extras: Partial<OwnedTerminalAttachmentDto> = {}): OwnedTerminalAttachmentDto => ({
  control: 'owned', controlEpoch: 1, tabId, sessionId: tabId, generation, firstSequence: 1, lastSequence: chunks.at(-1)?.sequence ?? 0,
  chunks: chunks.map(({ type, tabId, generation, sequence, data }) => ({ type, tabId, generation, sequence, data })),
  truncated: false, cols: 80, rows: 24, lifetime: 'app-owned', state: 'open', exitCode: null, ...extras,
});
const flush = async () => { for (let i = 0; i < 30; i++) await Promise.resolve(); };
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
}
function setup(attachment: TerminalAttachmentDto = snapshot()) {
  let emit: (event: TerminalEvent) => void = () => undefined;
  const unsubscribe = vi.fn(), messages: HostMessage[] = [], onError = vi.fn(), onFocus = vi.fn(), onState = vi.fn();
  const api = {
    attachTerminal: vi.fn<MobileTerminalApi['attachTerminal']>(async () => success(attachment)),
    writeTerminal: vi.fn<MobileTerminalApi['writeTerminal']>(async () => success({ written: true })),
    resizeTerminal: vi.fn<MobileTerminalApi['resizeTerminal']>(async () => success({ resized: true })),
    acknowledgeTerminal: vi.fn<MobileTerminalApi['acknowledgeTerminal']>(async () => success({ acknowledged: true })),
    detachTerminal: vi.fn<MobileTerminalApi['detachTerminal']>(async () => success({ detached: true })),
    takeTerminalControl: vi.fn<MobileTerminalApi['takeTerminalControl']>(async () => success({ taken: true })),
  };
  const subscribeTerminal = vi.fn((listener: (event: TerminalEvent) => void) => { emit = listener; return unsubscribe; });
  const options = { api, subscribeTerminal, tabId, viewId: 'page-1', fontSize: 14, onError, onFocus, onState, send: (message: HostMessage) => messages.push(message) };
  const bridge = new TerminalBridge(options);
  const latest = () => messages.filter((m): m is Extract<HostMessage, { type: 'output' }> => m.type === 'output').at(-1)!;
  const rendered = () => { const m = latest(); bridge.receive({ type: 'rendered', viewId: m.viewId, epoch: m.epoch, ticket: m.ticket }); };
  bridge.start();
  return { api, options, bridge, messages, onError, onFocus, onState, unsubscribe, latest, rendered, emit: (value: TerminalEvent) => emit(value), subscribeTerminal };
}
afterEach(() => vi.useRealTimers());
describe('mobile terminal delivery', () => {
  it('subscribes before attach, parses replay at its grid and ACKs only matching render receipts', async () => {
    const f = setup(snapshot([event(8, 'retained')], { firstSequence: 8, truncated: true, snapshot: true, cols: 132, rows: 43 }));
    await flush();
    expect(f.subscribeTerminal.mock.invocationCallOrder[0]).toBeLessThan(f.api.attachTerminal.mock.invocationCallOrder[0]!);
    expect(f.messages[0]).toMatchObject({ type: 'reset', cols: 132, rows: 43 });
    expect(f.latest()).toMatchObject({ data: 'retained', historical: true, fit: true });
    expect(f.api.acknowledgeTerminal).not.toHaveBeenCalled();
    f.bridge.receive({ type: 'rendered', viewId: 'wrong', epoch: f.latest().epoch, ticket: f.latest().ticket });
    await flush(); expect(f.api.acknowledgeTerminal).not.toHaveBeenCalled();
    f.rendered(); await flush();
    expect(f.api.acknowledgeTerminal).toHaveBeenLastCalledWith({ tabId, generation, controlEpoch: 1, sequence: 8 });
    f.bridge.dispose();
  });
  it('opens the native keyboard only after one matching explicit focus receipt', async () => {
    const f = setup(); await flush();
    const receipt = { type: 'focused' as const, viewId: 'page-1', epoch: f.latest().epoch };
    f.bridge.receive(receipt);
    expect(f.onFocus).not.toHaveBeenCalled();
    f.bridge.focus(true);
    expect(f.onFocus).not.toHaveBeenCalled();
    f.bridge.receive({ ...receipt, epoch: 'stale' });
    expect(f.onFocus).not.toHaveBeenCalled();
    f.bridge.receive(receipt); f.bridge.receive(receipt);
    expect(f.onFocus).toHaveBeenCalledOnce();
    f.bridge.focus(true); f.bridge.focus(false); f.bridge.receive(receipt);
    expect(f.onFocus).toHaveBeenCalledOnce();
    f.bridge.dispose();
  });
  it('applies theme after reset and updates it without detaching the shell', async () => {
    const f = setup();
    f.bridge.setTheme({ accentColor: '#123456', backgroundColor: '#ffffff' });
    await flush();
    expect(f.messages[0]).toMatchObject({ type: 'reset' });
    expect(f.messages[1]).toMatchObject({ type: 'theme', accentColor: '#123456', backgroundColor: '#ffffff' });
    f.bridge.setTheme({ accentColor: '#654321', backgroundColor: '#111016' });
    expect(f.messages.at(-1)).toMatchObject({ type: 'theme', accentColor: '#654321' });
    expect(f.api.attachTerminal).toHaveBeenCalledOnce();
    f.bridge.dispose();
  });
  it('queues early and live output behind replay, ignores duplicates, and releases contiguous credit', async () => {
    const f = setup(); f.emit(event(2, 'live')); await flush();
    expect(f.messages.filter(m => m.type === 'output')).toHaveLength(1);
    f.rendered(); await flush(); expect(f.latest().data).toBe('live');
    f.emit(event(2)); f.emit(event(3, 'next')); f.rendered(); await flush();
    expect(f.api.acknowledgeTerminal).toHaveBeenLastCalledWith({ tabId, generation, controlEpoch: 1, sequence: 2 });
    expect(f.latest().data).toBe('next'); f.rendered(); await flush();
    expect(f.api.acknowledgeTerminal).toHaveBeenLastCalledWith({ tabId, generation, controlEpoch: 1, sequence: 3 }); f.bridge.dispose();
  });
  it('detaches before sequence gap recovery and fences stale same-generation callbacks', async () => {
    vi.useFakeTimers(); const f = setup(); await flush(); const stale = f.latest();
    f.emit(event(3)); await vi.advanceTimersByTimeAsync(250); await flush();
    expect(f.api.detachTerminal.mock.invocationCallOrder[0]).toBeLessThan(f.api.attachTerminal.mock.invocationCallOrder[1]!);
    f.bridge.receive({ type: 'rendered', viewId: stale.viewId, epoch: stale.epoch, ticket: stale.ticket });
    await flush(); expect(f.api.acknowledgeTerminal).not.toHaveBeenCalled();
    f.rendered(); await flush(); expect(f.api.acknowledgeTerminal).toHaveBeenCalledOnce(); f.bridge.dispose();
  });
  it('keeps ownership without idle polling or resize renewal and recovers a failed rendered ACK', async () => {
    vi.useFakeTimers(); const f = setup(); await flush(); f.rendered(); await flush();
    f.bridge.receive({ type: 'resize', viewId: 'page-1', epoch: f.latest().epoch, cols: 90, rows: 30 }); await flush();
    await vi.advanceTimersByTimeAsync(60000); await flush();
    expect(f.api.resizeTerminal).toHaveBeenCalledOnce(); expect(f.api.acknowledgeTerminal).toHaveBeenCalledOnce();
    expect(f.onState).toHaveBeenLastCalledWith('attached'); expect(f.api.writeTerminal).not.toHaveBeenCalled();
    f.api.acknowledgeTerminal.mockResolvedValueOnce(failure('TARGET_LOST', 'lost delivery'));
    f.emit(event(2)); f.rendered(); await flush();
    await vi.advanceTimersByTimeAsync(250); await flush(); expect(f.api.attachTerminal).toHaveBeenCalledTimes(2); f.bridge.dispose();
  });
  it('does not acknowledge idle output before replay has rendered', async () => {
    vi.useFakeTimers(); const f = setup(); await flush();
    await vi.advanceTimersByTimeAsync(60000); await flush();
    expect(f.api.acknowledgeTerminal).not.toHaveBeenCalled(); expect(f.api.resizeTerminal).not.toHaveBeenCalled(); f.bridge.dispose();
  });
  it('uses xterm paste, bounds queued input, and never repeats failed writes', async () => {
    const f = setup(); await flush();
    f.bridge.paste('hello\n'); expect(f.messages.at(-1)).toMatchObject({ type: 'paste', data: 'hello\n' });
    f.bridge.input('é'.repeat(32769)); await flush(); expect(f.api.writeTerminal).not.toHaveBeenCalled();
    f.api.writeTerminal.mockResolvedValueOnce(failure('NATIVE_UNAVAILABLE', 'uncertain'));
    f.bridge.input('once'); await flush(); expect(f.api.writeTerminal).toHaveBeenCalledOnce();
    expect(f.onError).toHaveBeenCalledTimes(2); f.bridge.dispose();
  });
  it('cleans up late attachment responses before a remount can attach', async () => {
    const f = setup(); let resolve!: (value: ReturnType<typeof success<TerminalAttachmentDto>>) => void;
    f.api.attachTerminal.mockImplementationOnce(() => new Promise(done => { resolve = done; }));
    await flush(); f.bridge.dispose();
    const replacement = new TerminalBridge(f.options); replacement.start();
    resolve(success(snapshot())); await flush();
    expect(f.api.detachTerminal.mock.invocationCallOrder[0]).toBeLessThan(f.api.attachTerminal.mock.invocationCallOrder[1]!);
    expect(f.unsubscribe).toHaveBeenCalledOnce(); replacement.dispose();
  });
  it('waits for delayed detach before a rapid remount and rejects the retired render receipt', async () => {
    const f = setup(); await flush(); const retired = f.latest();
    const pending = deferred<Awaited<ReturnType<MobileTerminalApi['detachTerminal']>>>();
    f.api.detachTerminal.mockReturnValueOnce(pending.promise);
    f.bridge.dispose(); const replacement = new TerminalBridge(f.options); replacement.start(); await flush();
    expect(f.api.detachTerminal).toHaveBeenCalledOnce(); expect(f.api.attachTerminal).toHaveBeenCalledOnce();
    pending.resolve(success({ detached: true })); await flush();
    expect(f.api.attachTerminal).toHaveBeenCalledTimes(2);
    replacement.receive({ type: 'rendered', viewId: retired.viewId, epoch: retired.epoch, ticket: retired.ticket });
    await flush(); expect(f.api.acknowledgeTerminal).not.toHaveBeenCalled();
    const current = f.latest();
    replacement.receive({ type: 'rendered', viewId: current.viewId, epoch: current.epoch, ticket: current.ticket });
    await flush(); expect(f.api.acknowledgeTerminal).toHaveBeenCalledOnce(); replacement.dispose();
  });
  it('waits for an in-flight ACK before detach and remount', async () => {
    const f = setup(); await flush();
    const pending = deferred<Awaited<ReturnType<MobileTerminalApi['acknowledgeTerminal']>>>();
    f.api.acknowledgeTerminal.mockReturnValueOnce(pending.promise);
    f.rendered(); await flush(); expect(f.api.acknowledgeTerminal).toHaveBeenCalledOnce();
    f.bridge.dispose(); const replacement = new TerminalBridge(f.options); replacement.start(); await flush();
    expect(f.api.detachTerminal).not.toHaveBeenCalled(); expect(f.api.attachTerminal).toHaveBeenCalledOnce();
    pending.resolve(success({ acknowledged: true })); await flush();
    expect(f.api.detachTerminal).toHaveBeenCalledOnce(); expect(f.api.attachTerminal).toHaveBeenCalledTimes(2);
    replacement.dispose();
  });
  it('retains failed unmount cleanup and retries it before a replacement can attach', async () => {
    vi.useFakeTimers(); const f = setup(); await flush();
    f.api.detachTerminal.mockResolvedValueOnce(failure('NATIVE_UNAVAILABLE', 'offline'))
      .mockResolvedValueOnce(failure('NATIVE_UNAVAILABLE', 'still offline'));
    f.bridge.dispose(); const replacement = new TerminalBridge(f.options); replacement.start(); await flush();
    expect(f.api.detachTerminal).toHaveBeenCalledTimes(2); expect(f.api.attachTerminal).toHaveBeenCalledOnce();
    expect(f.onError).toHaveBeenCalledOnce();
    await vi.advanceTimersByTimeAsync(10000); await flush();
    expect(f.api.detachTerminal).toHaveBeenCalledTimes(3); expect(f.api.attachTerminal).toHaveBeenCalledTimes(2);
    expect(f.api.detachTerminal.mock.invocationCallOrder[2]).toBeLessThan(f.api.attachTerminal.mock.invocationCallOrder[1]!);
    replacement.dispose();
  });
  it('clears a denied view and resumes input only after an explicit ownership grant', async () => {
    vi.useFakeTimers(); const f = setup(); await flush(); f.rendered(); await flush();
    f.bridge.receive({ type: 'resize', viewId: 'page-1', epoch: f.latest().epoch, cols: 80, rows: 24 }); await flush();
    f.api.writeTerminal.mockResolvedValueOnce(failure('TERMINAL_CONTROL_REQUIRED', 'busy'));
    f.bridge.input('once'); await flush(); await vi.advanceTimersByTimeAsync(10000); await flush();
    expect(f.api.resizeTerminal).toHaveBeenCalledOnce(); expect(f.api.writeTerminal).toHaveBeenCalledOnce();
    expect(f.api.attachTerminal).toHaveBeenCalledOnce();
    f.emit(event(2, 'not for this view')); f.rendered(); f.bridge.input('blocked'); await flush();
    expect(f.onState).toHaveBeenLastCalledWith('blocked'); expect(f.messages.at(-1)).toMatchObject({ type: 'clear' });
    expect(f.api.acknowledgeTerminal).toHaveBeenCalledOnce(); expect(f.api.writeTerminal).toHaveBeenCalledOnce();
    f.api.attachTerminal.mockResolvedValueOnce(success(snapshot(undefined, { controlEpoch: 2 })));
    f.emit({ type: 'control', tabId, generation, control: 'owned', controlEpoch: 2 }); await flush();
    f.bridge.input('explicit retry'); await flush();
    expect(f.api.writeTerminal).toHaveBeenLastCalledWith({ tabId, generation, controlEpoch: 2, data: 'explicit retry' });
    f.bridge.dispose();
  });
  it.each(['writeTerminal', 'resizeTerminal'] as const)('recovers a stale identity from %s without repeating input', async method => {
    vi.useFakeTimers(); const f = setup(); await flush();
    f.api[method].mockResolvedValueOnce(failure('TARGET_LOST', 'stale generation'));
    if (method === 'writeTerminal') f.bridge.input('do not repeat');
    else f.bridge.receive({ type: 'resize', viewId: 'page-1', epoch: f.latest().epoch, cols: 80, rows: 24 });
    await flush(); await vi.advanceTimersByTimeAsync(250); await flush();
    expect(f.api.attachTerminal).toHaveBeenCalledTimes(2); expect(f.api[method]).toHaveBeenCalledOnce();
    expect(f.onError).toHaveBeenCalledOnce(); f.bridge.dispose();
  });
  it('does not report a delayed write error against a new attachment epoch', async () => {
    vi.useFakeTimers(); const f = setup(); await flush();
    const pending = deferred<Awaited<ReturnType<MobileTerminalApi['writeTerminal']>>>();
    f.api.writeTerminal.mockReturnValueOnce(pending.promise); f.bridge.input('old'); await flush();
    const nextGeneration = '12345678-1234-4234-8234-123456789abe';
    const next = { ...event(1, 'new'), generation: nextGeneration };
    f.api.attachTerminal.mockResolvedValueOnce(success(snapshot([next], { generation: nextGeneration })));
    f.emit(next); await vi.advanceTimersByTimeAsync(250); await flush();
    pending.resolve(failure('TARGET_LOST', 'retired')); await flush();
    expect(f.api.attachTerminal).toHaveBeenCalledTimes(2); expect(f.onError).not.toHaveBeenCalled();
    f.bridge.dispose();
  });
  it('rejects an internally gapped replay without releasing credit', async () => {
    const f = setup(snapshot([event(1), event(3)])); await flush();
    expect(f.onError).toHaveBeenCalled(); expect(f.messages).toEqual([expect.objectContaining({ type: 'clear' })]);
    expect(f.api.acknowledgeTerminal).not.toHaveBeenCalled(); f.bridge.dispose();
  });
  it('selects generations from attachments and ignores delayed retired-generation events', async () => {
    vi.useFakeTimers(); const f = setup(); await flush();
    const nextGeneration = '12345678-1234-4234-8234-123456789abe';
    const next = { ...event(1, 'new'), generation: nextGeneration };
    f.api.attachTerminal.mockResolvedValueOnce(success(snapshot([next], { generation: nextGeneration })));
    f.emit(next); await vi.advanceTimersByTimeAsync(250); await flush();
    expect(f.latest().data).toBe('new');
    f.emit(event(2, 'retired')); await vi.advanceTimersByTimeAsync(250); await flush();
    expect(f.api.attachTerminal).toHaveBeenCalledTimes(2);
    f.rendered(); await flush();
    expect(f.api.acknowledgeTerminal).toHaveBeenLastCalledWith({ tabId, generation: nextGeneration, controlEpoch: 1, sequence: 1 });
    f.bridge.dispose();
  });
  it('forces a real resize for a truncated raw TUI tail, but not for a snapshot', async () => {
    vi.useFakeTimers(); const f = setup(snapshot([event(8, 'tail')], { firstSequence: 8, truncated: true }));
    await flush(); f.rendered(); await flush();
    f.bridge.receive({ type: 'resize', viewId: 'page-1', epoch: f.latest().epoch, cols: 80, rows: 24 }); await flush();
    expect(f.api.resizeTerminal).toHaveBeenLastCalledWith({ tabId, generation, controlEpoch: 1, cols: 80, rows: 23 });
    await vi.advanceTimersByTimeAsync(60); await flush();
    expect(f.api.resizeTerminal).toHaveBeenLastCalledWith({ tabId, generation, controlEpoch: 1, cols: 80, rows: 24 });
    f.bridge.dispose();
  });
  it('coalesces queued keyboard resize events without automatic idle resizing', async () => {
    vi.useFakeTimers(); const f = setup(); await flush();
    f.api.resizeTerminal.mockResolvedValueOnce(failure('TERMINAL_CONTROL_REQUIRED', 'busy'));
    const epoch = f.latest().epoch;
    for (let rows = 20; rows <= 30; rows++) f.bridge.receive({ type: 'resize', viewId: 'page-1', epoch, cols: 80, rows });
    await flush(); expect(f.api.resizeTerminal).toHaveBeenCalledOnce();
    expect(f.api.resizeTerminal).toHaveBeenLastCalledWith({ tabId, generation, controlEpoch: 1, cols: 80, rows: 30 });
    await vi.advanceTimersByTimeAsync(10000); await flush(); expect(f.api.resizeTerminal).toHaveBeenCalledOnce();
    f.bridge.dispose();
  });
  it('reports typed terminal errors and local attachment states independently of the client poll', async () => {
    vi.useFakeTimers(); const f = setup(); await flush();
    expect(f.onState.mock.calls.flat()).toEqual(['attaching', 'attached']);
    f.api.writeTerminal.mockResolvedValueOnce(failure('TARGET_LOST', 'stale delivery', true));
    f.bridge.input('once'); await flush();
    expect(f.onError.mock.calls[0]?.[0]).toBeInstanceOf(TerminalOperationError);
    expect(f.onError.mock.calls[0]?.[0].detail).toMatchObject({ code: 'TARGET_LOST' });
    expect(f.onState).toHaveBeenLastCalledWith('reconnecting');
    await vi.advanceTimersByTimeAsync(250); await flush();
    expect(f.onState).toHaveBeenLastCalledWith('attached');
    f.emit({ type: 'exit', tabId, generation, controlEpoch: 1, lastSequence: 1, exitCode: 0, signal: null });
    expect(f.onState).toHaveBeenLastCalledWith('closed');
    f.bridge.dispose();
  });
  it('coalesces pending adjacent input without changing bytes or replaying historical output', async () => {
    const f = setup(snapshot([event(1, '\x03\x03')])); await flush();
    expect(f.api.writeTerminal).not.toHaveBeenCalled();
    const pending = deferred<Awaited<ReturnType<MobileTerminalApi['writeTerminal']>>>();
    f.api.writeTerminal.mockReturnValueOnce(pending.promise);
    f.bridge.input('first'); await flush();
    f.bridge.input('\x03'); f.bridge.input('\x03'); f.bridge.input('tail');
    await flush(); expect(f.api.writeTerminal).toHaveBeenCalledOnce();
    pending.resolve(success({ written: true })); await flush();
    expect(f.api.writeTerminal.mock.calls.map(([input]) => input.data)).toEqual(['first', '\x03\x03tail']);
    f.bridge.dispose();
  });
  it('does not coalesce writes across an intervening resize', async () => {
    const f = setup(); await flush();
    f.bridge.input('before');
    f.bridge.receive({ type: 'resize', viewId: 'page-1', epoch: f.latest().epoch, cols: 100, rows: 30 });
    f.bridge.input('after'); await flush();
    expect(f.api.writeTerminal.mock.calls.map(([input]) => input.data)).toEqual(['before', 'after']);
    expect(f.api.writeTerminal.mock.invocationCallOrder[0]).toBeLessThan(f.api.resizeTerminal.mock.invocationCallOrder[0]!);
    expect(f.api.resizeTerminal.mock.invocationCallOrder[0]).toBeLessThan(f.api.writeTerminal.mock.invocationCallOrder[1]!);
    f.bridge.dispose();
  });
  it('sends validated keys to the page and leaves encoded repeated interrupts unchanged', async () => {
    const f = setup(); await flush();
    const modifiers = { ...emptyModifiers, ctrl: true };
    f.bridge.setModifiers(modifiers);
    f.bridge.key({ key: 'c', modifiers }); f.bridge.key({ key: 'c', modifiers });
    expect(f.messages.filter(message => message.type === 'key')).toMatchObject([
      { event: { key: 'c', modifiers } }, { event: { key: 'c', modifiers } },
    ]);
    expect(f.api.writeTerminal).not.toHaveBeenCalled();
    f.bridge.receive({ type: 'input', viewId: 'page-1', epoch: f.latest().epoch, data: '\x03\x03' }); await flush();
    expect(f.api.writeTerminal).toHaveBeenCalledExactlyOnceWith({ tabId, generation, controlEpoch: 1, data: '\x03\x03' });
    f.bridge.dispose(); f.bridge.key({ key: 'c', modifiers });
    expect(f.messages.filter(message => message.type === 'key')).toHaveLength(2);
  });
  it('bounds pending ACKs and coalesces to the latest rendered sequence', async () => {
    const f = setup(); await flush();
    const pending = deferred<Awaited<ReturnType<MobileTerminalApi['acknowledgeTerminal']>>>();
    f.api.acknowledgeTerminal.mockReturnValueOnce(pending.promise);
    f.rendered(); await flush();
    for (let sequence = 2; sequence <= 100; sequence++) {
      f.emit(event(sequence)); f.rendered(); await flush();
    }
    expect(f.api.acknowledgeTerminal).toHaveBeenCalledOnce();
    pending.resolve(success({ acknowledged: true })); await flush();
    expect(f.api.acknowledgeTerminal).toHaveBeenCalledTimes(2);
    expect(f.api.acknowledgeTerminal).toHaveBeenLastCalledWith({ tabId, generation, controlEpoch: 1, sequence: 100 });
    f.bridge.dispose();
  });
  it('bounds live queues and reattaches rather than ACKing dropped output', async () => {
    vi.useFakeTimers(); const f = setup(); await flush();
    for (let i = 2; i < 22; i++) f.emit(event(i, 'x'.repeat(16384)));
    await vi.advanceTimersByTimeAsync(250); await flush();
    expect(f.api.attachTerminal).toHaveBeenCalledTimes(2); expect(f.api.acknowledgeTerminal).not.toHaveBeenCalled(); f.bridge.dispose();
  });
});
