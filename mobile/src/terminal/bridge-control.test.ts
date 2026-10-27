import { afterEach, expect, it, vi } from 'vitest';
import { failure, success, type OwnedTerminalAttachmentDto, type TerminalAttachmentDto, type TerminalEvent } from '../../../src/shared/contracts';
import { TerminalBridge, type MobileTerminalApi } from './bridge';
import type { HostMessage } from './protocol';
import { deferred, flush, ids } from '../client/test-fixtures';
const tabId = ids.tab, generation = ids.generation;
const owned = (controlEpoch = 1, data = 'private screen'): OwnedTerminalAttachmentDto => ({
  control: 'owned', controlEpoch, tabId, sessionId: ids.session, generation, firstSequence: 1, lastSequence: 1,
  chunks: [{ type: 'data', tabId, generation, sequence: 1, data }], snapshot: true, truncated: false,
  cols: 115, rows: 32, state: 'open', exitCode: null, lifetime: 'app-owned',
});
const blocked = (controlEpoch = 2): TerminalAttachmentDto => ({ control: 'blocked', controlEpoch, tabId, sessionId: ids.session, generation });
const cleanups: Array<() => void> = [];
afterEach(() => { cleanups.splice(0).reverse().forEach(cleanup => cleanup()); vi.useRealTimers(); });
function fixture(attachment: TerminalAttachmentDto = owned()) {
  vi.useFakeTimers();
  const messages: HostMessage[] = [], onError = vi.fn(), onState = vi.fn();
  let emit: (event: TerminalEvent) => void = () => {};
  const api = {
    attachTerminal: vi.fn<MobileTerminalApi['attachTerminal']>(async () => success(attachment)),
    takeTerminalControl: vi.fn<MobileTerminalApi['takeTerminalControl']>(async () => success({ taken: true })),
    writeTerminal: vi.fn<MobileTerminalApi['writeTerminal']>(async () => success({ written: true })),
    resizeTerminal: vi.fn<MobileTerminalApi['resizeTerminal']>(async () => success({ resized: true })),
    acknowledgeTerminal: vi.fn<MobileTerminalApi['acknowledgeTerminal']>(async () => success({ acknowledged: true })),
    detachTerminal: vi.fn<MobileTerminalApi['detachTerminal']>(async () => success({ detached: true })),
  };
  const bridge = new TerminalBridge({ api, tabId, viewId: 'control', fontSize: 14, onError, onState,
    subscribeTerminal: listener => { emit = listener; return () => {}; }, send: message => messages.push(message) });
  cleanups.push(() => bridge.dispose()); bridge.start();
  const latest = () => messages.filter(message => message.type === 'output').at(-1);
  const rendered = () => { const output = latest(); if (output) bridge.receive({ type: 'rendered', viewId: 'control', epoch: output.epoch, ticket: output.ticket }); };
  return { api, bridge, messages, onError, onState, latest, rendered, emit: (event: TerminalEvent) => emit(event) };
}
it('keeps a blocked subscription blank with no grid, input, ACK or idle polling until explicit Take it', async () => {
  const f = fixture(blocked()); await flush();
  expect(f.onState).toHaveBeenLastCalledWith('blocked'); expect(f.messages).toEqual([expect.objectContaining({ type: 'clear' })]);
  f.bridge.input('forbidden'); f.bridge.paste('forbidden'); f.bridge.focus(true);
  await vi.advanceTimersByTimeAsync(600_000); await flush();
  expect(f.api.attachTerminal).toHaveBeenCalledOnce(); expect(f.api.writeTerminal).not.toHaveBeenCalled();
  expect(f.api.resizeTerminal).not.toHaveBeenCalled(); expect(f.api.acknowledgeTerminal).not.toHaveBeenCalled();
  f.api.attachTerminal.mockResolvedValueOnce(success(owned(3, 'fresh owner screen')));
  await f.bridge.takeControl(); await flush();
  expect(f.api.takeTerminalControl).toHaveBeenCalledExactlyOnceWith({ tabId, generation });
  expect(f.api.detachTerminal).not.toHaveBeenCalled();
  expect(f.latest()).toMatchObject({ historical: true, data: 'fresh owner screen' });
  f.rendered(); await flush();
  expect(f.api.acknowledgeTerminal).toHaveBeenLastCalledWith({ tabId, generation, controlEpoch: 3, sequence: 1 });
});
it('clears contents immediately on loss, fences old render receipts and queued writes, and does not detach the waiting viewer', async () => {
  const f = fixture(); await flush(); const old = f.latest();
  const write = deferred<Awaited<ReturnType<MobileTerminalApi['writeTerminal']>>>();
  f.api.writeTerminal.mockReturnValueOnce(write.promise);
  f.bridge.input('in-flight'); await flush(); f.bridge.input('queued');
  f.emit({ type: 'control', tabId, generation, control: 'blocked', controlEpoch: 2 });
  expect(f.messages.at(-1)).toMatchObject({ type: 'clear' }); expect(f.onState).toHaveBeenLastCalledWith('blocked');
  if (old) f.bridge.receive({ type: 'rendered', viewId: 'control', epoch: old.epoch, ticket: old.ticket });
  f.emit({ type: 'data', tabId, generation, controlEpoch: 1, sequence: 2, data: 'stale secret' });
  write.resolve(failure('TERMINAL_CONTROL_REQUIRED', 'Ownership changed.')); await flush();
  expect(f.api.writeTerminal).toHaveBeenCalledExactlyOnceWith({ tabId, generation, controlEpoch: 1, data: 'in-flight' });
  expect(f.api.acknowledgeTerminal).not.toHaveBeenCalled(); expect(f.api.detachTerminal).not.toHaveBeenCalled();
  expect(f.messages.filter(message => message.type === 'output')).toHaveLength(1); expect(f.onError).not.toHaveBeenCalled();
});
it('waits for a fresh full snapshot after grant and never paints old-epoch targeted frames', async () => {
  const f = fixture(blocked()); await flush();
  const fresh = deferred<Awaited<ReturnType<MobileTerminalApi['attachTerminal']>>>();
  f.api.attachTerminal.mockReturnValueOnce(fresh.promise);
  f.emit({ type: 'control', tabId, generation, control: 'owned', controlEpoch: 3 }); await flush();
  f.bridge.input('too early'); expect(f.api.writeTerminal).not.toHaveBeenCalled(); expect(f.latest()).toBeUndefined();
  f.emit({ type: 'data', tabId, generation, controlEpoch: 2, sequence: 2, data: 'old epoch' });
  fresh.resolve(success(owned(3, 'new snapshot'))); await flush();
  expect(f.latest()).toMatchObject({ historical: true, data: 'new snapshot' });
  expect(f.messages.filter(message => message.type === 'reset')).toEqual([expect.objectContaining({ cols: 115, rows: 32 })]);
  f.rendered(); await flush(); f.bridge.input('new owner'); await flush();
  expect(f.api.writeTerminal).toHaveBeenLastCalledWith({ tabId, generation, controlEpoch: 3, data: 'new owner' });
  expect(f.api.detachTerminal).not.toHaveBeenCalled(); expect(f.onError).not.toHaveBeenCalled();
});
it('does not paint a delayed attachment response after a newer ownership-loss event', async () => {
  const f = fixture(), pending = deferred<Awaited<ReturnType<MobileTerminalApi['attachTerminal']>>>();
  f.api.attachTerminal.mockReturnValueOnce(pending.promise); await flush();
  f.emit({ type: 'control', tabId, generation, control: 'blocked', controlEpoch: 2 });
  pending.resolve(success(owned(1, 'must not paint'))); await flush();
  expect(f.latest()).toBeUndefined(); expect(f.onState).toHaveBeenLastCalledWith('blocked');
  expect(f.api.detachTerminal).not.toHaveBeenCalled();
});
