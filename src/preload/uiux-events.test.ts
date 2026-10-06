import { expect, it, vi } from 'vitest';
import { randomUUID } from 'node:crypto';
import type { ManagerApi } from '../shared/contracts';
const bridge = vi.hoisted(() => ({ api: undefined as ManagerApi | undefined, listeners: new Map<string, (e: unknown, value: unknown) => void>() }));
vi.mock('electron', () => ({ contextBridge: { exposeInMainWorld: (_name: string, api: ManagerApi) => { bridge.api = api; } }, ipcRenderer: {
  invoke: vi.fn(), on: (channel: string, listener: (e: unknown, value: unknown) => void) => bridge.listeners.set(channel, listener),
  removeListener: (channel: string) => bridge.listeners.delete(channel),
} }));
import './index';
it('validates and delivers activity without generation or output sequence fields', () => {
  const receive = vi.fn(), stop = bridge.api!.subscribeTerminal!(receive), tabId = randomUUID();
  const send = (value: unknown) => bridge.listeners.get('manager:terminal')?.({}, value);
  send({ type: 'activity', tabId, busy: true }); send({ type: 'activity', tabId, busy: false });
  expect(receive.mock.calls).toEqual([[{ type: 'activity', tabId, busy: true }], [{ type: 'activity', tabId, busy: false }]]);
  for (const event of [{ type: 'activity', tabId, busy: 'true' }, { type: 'activity', tabId: 'bad', busy: true }, { type: 'activity', tabId, busy: true, generation: randomUUID() }]) send(event);
  expect(receive).toHaveBeenCalledTimes(2); stop();
});
it('preserves optional selectSessionId and rejects malformed changed events', () => {
  const receive = vi.fn(), stop = bridge.api!.subscribe(receive), selectSessionId = randomUUID();
  const send = (value: unknown) => bridge.listeners.get('manager:changed')?.({}, value);
  send({ revision: 1, reason: 'sessions', selectSessionId }); send({ revision: 2, reason: 'native' });
  expect(receive.mock.calls).toEqual([[{ revision: 1, reason: 'sessions', selectSessionId }], [{ revision: 2, reason: 'native' }]]);
  send({ revision: 3, reason: 'sessions', selectSessionId: 'bad' }); send({ revision: 3, reason: 'sessions', selectSessionId, unknown: true });
  expect(receive).toHaveBeenCalledTimes(2); stop();
});
