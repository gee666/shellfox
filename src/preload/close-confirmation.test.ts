import { beforeEach, expect, it, vi } from 'vitest';
import { randomUUID } from 'node:crypto';
import { closeConfirmationApi as api } from './close-confirmation';
const bridge = vi.hoisted(() => ({ invoke: vi.fn(), listeners: new Set<(event: unknown, value: unknown) => void>(), removed: vi.fn() }));
vi.mock('electron', () => ({ ipcRenderer: {
  invoke: bridge.invoke,
  on: (_channel: string, listener: (event: unknown, value: unknown) => void) => bridge.listeners.add(listener),
  removeListener: (_channel: string, listener: (event: unknown, value: unknown) => void) => { bridge.listeners.delete(listener); bridge.removed(); },
} }));
beforeEach(() => { bridge.invoke.mockReset(); bridge.listeners.clear(); bridge.removed.mockReset(); });
it('validates responses and only exposes a pending-id approval, not a quit command', async () => {
  const id = randomUUID();
  expect(await api.respondCloseConfirmation({ id: 'bad', approved: true })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(bridge.invoke).not.toHaveBeenCalled();
  bridge.invoke.mockResolvedValueOnce({ ok: true, value: { id, kind: 'quit', terminalCount: 2 } });
  expect(await api.getCloseConfirmation()).toEqual({ ok: true, value: { id, kind: 'quit', terminalCount: 2 } });
  expect(bridge.invoke).toHaveBeenLastCalledWith('app:confirmation:get', {});
  bridge.invoke.mockResolvedValueOnce({ ok: true, value: { accepted: true } });
  expect(await api.respondCloseConfirmation({ id, approved: false })).toEqual({ ok: true, value: { accepted: true } });
  expect(bridge.invoke).toHaveBeenLastCalledWith('app:confirmation:respond', { id, approved: false });
  bridge.invoke.mockResolvedValueOnce({ ok: true, value: { accepted: true, quit: true } });
  expect(await api.respondCloseConfirmation({ id, approved: true })).toMatchObject({ ok: false, error: { code: 'INTERNAL' } });
  bridge.invoke.mockRejectedValue(new Error('Disconnected'));
  expect(await api.getCloseConfirmation()).toMatchObject({ ok: false, error: { code: 'INTERNAL' } });
  expect(await api.respondCloseConfirmation({ id, approved: false })).toMatchObject({ ok: false, error: { code: 'INTERNAL' } });
});
it('filters malformed notifications, accepts null clears, and unsubscribes only its listener', () => {
  const receive = vi.fn(), other = vi.fn();
  const stop = api.subscribeCloseConfirmation(receive), stopOther = api.subscribeCloseConfirmation(other);
  const emit = (value: unknown) => bridge.listeners.forEach(listener => listener({}, value));
  for (const value of [{}, { id: randomUUID(), kind: 'quit', terminalCount: -1 }, { id: randomUUID(), kind: 'quit', terminalCount: 1, command: 'quit' }]) emit(value);
  expect(receive).not.toHaveBeenCalled();
  const confirmation = { id: randomUUID(), kind: 'update', terminalCount: 0 };
  emit(confirmation); emit(null);
  expect(receive.mock.calls).toEqual([[confirmation], [null]]);
  stop(); emit(null); expect(receive).toHaveBeenCalledTimes(2); expect(other).toHaveBeenCalledTimes(3);
  stopOther(); expect(bridge.listeners.size).toBe(0);
  api.subscribeCloseConfirmation(null as never)(); expect(bridge.listeners.size).toBe(0);
});
