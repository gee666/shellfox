import { beforeEach, expect, it, vi } from 'vitest';
import { randomUUID } from 'node:crypto';
const bridge = vi.hoisted(() => ({ invoke: vi.fn(), listeners: new Map<string, Set<(event: unknown, value: unknown) => void>>() }));
vi.mock('electron', () => ({ ipcRenderer: {
  invoke: bridge.invoke,
  on: (channel: string, listener: (event: unknown, value: unknown) => void) => { const current = bridge.listeners.get(channel) ?? new Set(); current.add(listener); bridge.listeners.set(channel, current); },
  removeListener: (channel: string, listener: (event: unknown, value: unknown) => void) => { bridge.listeners.get(channel)?.delete(listener); },
} }));
import { remoteApi } from './remote';
const state = { enabled: false, hostname: 'shellfox', status: 'disabled', loginUrl: null, error: null, peers: [], approvals: [], pending: [] };
beforeEach(() => { bridge.invoke.mockReset(); bridge.listeners.clear(); });
it('keeps remote requests on their own channel and validates requests and responses', async () => {
  expect(await remoteApi.configure({ enabled: true, hostname: '../invalid' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await remoteApi.request({ peerId: 'host', method: 'writeTerminal', payload: { tabId: randomUUID(), generation: randomUUID(), data: '\0' } })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(bridge.invoke).not.toHaveBeenCalled();
  bridge.invoke.mockResolvedValueOnce({ ok: true, value: state });
  expect(await remoteApi.configure({ enabled: false, hostname: 'shellfox' })).toEqual({ ok: true, value: state });
  expect(bridge.invoke).toHaveBeenLastCalledWith('remote:request', { method: 'configure', payload: { enabled: false, hostname: 'shellfox' } });
  bridge.invoke.mockResolvedValueOnce({ ok: true, value: { ...state, secret: 'not allowed' } });
  expect(await remoteApi.getState()).toMatchObject({ ok: false, error: { code: 'INTERNAL' } });
  bridge.invoke.mockResolvedValueOnce({ ok: true, value: { written: true, secret: 'not allowed' } });
  expect(await remoteApi.request({ peerId: 'host', method: 'writeTerminal', payload: { tabId: randomUUID(), generation: randomUUID(), data: 'pwd\r' } })).toMatchObject({ ok: false, error: { code: 'INTERNAL' } });
});
it('accepts ephemeral auth keys only as login inputs and handles transport errors', async () => {
  bridge.invoke.mockResolvedValueOnce({ ok: true, value: state });
  await remoteApi.login({ authKey: ['tskey', 'auth', 'synthetic-secret'].join('-') });
  expect(bridge.invoke).toHaveBeenLastCalledWith('remote:request', { method: 'login', payload: { authKey: ['tskey', 'auth', 'synthetic-secret'].join('-') } });
  bridge.invoke.mockRejectedValueOnce(new Error('gone'));
  expect(await remoteApi.getState()).toMatchObject({ ok: false, error: { code: 'INTERNAL', retryable: true } });
});
it('validates notifications and unsubscribes one listener without affecting another', () => {
  const a = vi.fn(), b = vi.fn(), stopA = remoteApi.subscribe(a), stopB = remoteApi.subscribe(b);
  const emit = (value: unknown) => { for (const listener of bridge.listeners.get('remote:event') ?? []) listener({}, value); };
  emit({ type: 'state', state: { ...state, backendSecret: 'bad' } });
  emit({ type: 'terminal', peerId: 'host', event: { type: 'data', tabId: 'bad', generation: randomUUID(), sequence: 1, data: 'bad' } });
  expect(a).not.toHaveBeenCalled();
  emit({ type: 'state', state }); stopA(); emit({ type: 'state', state });
  expect(a).toHaveBeenCalledOnce(); expect(b).toHaveBeenCalledTimes(2); stopB();
  expect(bridge.listeners.get('remote:event')?.size).toBe(0);
});
