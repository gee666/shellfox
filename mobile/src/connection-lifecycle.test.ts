// @vitest-environment jsdom
import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { RemoteClientError } from './client/protocol';
import { deferred, flush, TestTransport } from './client/test-fixtures';
import type { NetworkPeer } from './network';
import { failure, success } from '../../src/shared/contracts';
import { snapshot } from '../../src/renderer/test-fixtures';
const mocks = vi.hoisted(() => ({
  status: vi.fn(), start: vi.fn(), request: vi.fn(), save: vi.fn<() => Promise<void>>(), peers: vi.fn<() => Promise<NetworkPeer[]>>(),
  appState: { currentState: 'active', addEventListener: vi.fn() },
}));
vi.mock('react-native', () => ({ AppState: mocks.appState }));
vi.mock('./network', () => ({
  nativeNetwork: { start: mocks.start, request: mocks.request },
  networkStatus: mocks.status,
  discoverHosts: async () => ({ peers: await mocks.peers(), discovering: false }),
}));
vi.mock('./preferences', () => ({
  defaultPreferences: { hostname: 'test-phone', remembered: [], fontSize: 14 },
  loadPreferences: async () => ({ hostname: 'test-phone', remembered: [], fontSize: 14 }),
  savePreferences: mocks.save,
}));
const { useShellfox } = await import('./useShellfox');
const online = { status: 'online', loginUrl: null, error: null };
let changeState = (_state: string) => {};
beforeEach(() => {
  vi.resetAllMocks(); vi.useFakeTimers();
  mocks.appState.addEventListener.mockImplementation((_event: string, listener: (state: string) => void) => { changeState = listener; return { remove: vi.fn() }; });
  mocks.start.mockResolvedValue(''); mocks.status.mockResolvedValue(online); mocks.peers.mockResolvedValue([]); mocks.save.mockResolvedValue();
});
afterEach(() => { cleanup(); vi.useRealTimers(); });
it('recovers from a native status failure without leaving a connection-failed toast', async () => {
  mocks.status.mockRejectedValueOnce(new Error('Temporary connection failure'));
  const { result } = renderHook(useShellfox);
  await act(async () => { await flush(); });
  expect(result.current.network.status).toBe('error');
  expect(result.current.error).toBeNull();
  await act(async () => { await vi.advanceTimersByTimeAsync(250); await flush(); });
  expect(result.current.network).toEqual(online);
  expect(result.current.error).toBeNull();
});
it('keeps a dark-screen in-flight approval request, resumes its UUID, and withdraws only on explicit cancel', async () => {
  const peer: NetworkPeer = { id: 'host-1', hostname: 'workstation', online: true, addresses: [] };
  const transport = new TestTransport(), pair = deferred<string>();
  transport.pair = () => pair.promise; mocks.request.mockImplementation(transport.request); mocks.peers.mockResolvedValue([peer]);
  const { result } = renderHook(useShellfox); await act(async () => { await flush(); });
  await act(async () => { await result.current.connect(peer); await flush(); });
  const attempt = transport.calls('/v1/pair')[0];
  await act(async () => { changeState('inactive'); changeState('background'); await flush(); });
  pair.resolve(JSON.stringify(failure('APPROVAL_REQUIRED', 'Approve on host.', true)));
  await act(async () => { await flush(); await vi.advanceTimersByTimeAsync(180_000); });
  expect(result.current.machines[0].state.status).toBe('awaiting-approval');
  expect(transport.calls('/v1/disconnect')).toHaveLength(0); expect(transport.calls('/v1/request')).toHaveLength(0);
  expect(transport.calls('/v1/pair')).toEqual([attempt]);
  await act(async () => { changeState('active'); await flush(); });
  expect(transport.calls('/v1/pair')).toEqual([attempt, attempt]);
  await act(async () => { await result.current.disconnect(peer.id); await flush(); });
  expect(result.current.preferences.remembered).toEqual([]); expect(result.current.machines[0].state.status).toBe('disconnected');
  expect(transport.calls('/v1/disconnect')).toContain(attempt);
  await act(async () => { await vi.advanceTimersByTimeAsync(6000); });
  expect(transport.calls('/v1/pair')).toHaveLength(2);
});
it('does not automatically recreate an explicitly cancelled request when saving preferences fails', async () => {
  const peer: NetworkPeer = { id: 'host-1', hostname: 'workstation', online: true, addresses: [] };
  const transport = new TestTransport();
  transport.pair = async () => JSON.stringify(failure('APPROVAL_REQUIRED', 'Approve on host.', true));
  mocks.request.mockImplementation(transport.request); mocks.peers.mockResolvedValue([peer]);
  const { result } = renderHook(useShellfox); await act(async () => { await flush(); });
  await act(async () => { await result.current.connect(peer); await flush(); });
  mocks.save.mockRejectedValueOnce(new Error('Storage unavailable'));
  await act(async () => { await expect(result.current.disconnect(peer.id)).rejects.toThrow('Storage unavailable'); await flush(); });
  await act(async () => { await vi.advanceTimersByTimeAsync(6000); });
  expect(transport.calls('/v1/pair')).toHaveLength(1);
  expect(result.current.machines[0].state.status).toBe('disconnected');
  await act(async () => { await result.current.connect(peer); await flush(); });
  expect(transport.calls('/v1/pair')).toHaveLength(2);
  expect(transport.calls('/v1/pair')[1]).not.toBe(transport.calls('/v1/pair')[0]);
});
it('preserves a pending approval UUID across recoverable native health errors and resumes it online', async () => {
  const peer: NetworkPeer = { id: 'host-1', hostname: 'workstation', online: true, addresses: [] };
  const transport = new TestTransport(); transport.pair = async () => JSON.stringify(failure('APPROVAL_REQUIRED', 'Approve on host.', true));
  mocks.request.mockImplementation(transport.request); mocks.peers.mockResolvedValue([peer]);
  const { result } = renderHook(useShellfox); await act(async () => { await flush(); });
  await act(async () => { await result.current.connect(peer); await flush(); });
  const attempt = transport.calls('/v1/pair')[0], peerReads = mocks.peers.mock.calls.length;
  mocks.status.mockResolvedValue({ status: 'error', loginUrl: null, error: 'Control connection unavailable. Reconnecting automatically.' });
  await act(async () => { await vi.advanceTimersByTimeAsync(6000); });
  expect(result.current.machines[0].state.status).toBe('awaiting-approval');
  expect(transport.calls('/v1/disconnect')).toHaveLength(0); expect(transport.calls('/v1/pair')).toEqual([attempt]);
  expect(mocks.peers).toHaveBeenCalledTimes(peerReads);
  mocks.status.mockResolvedValue(online);
  await act(async () => { await vi.advanceTimersByTimeAsync(3250); });
  expect(transport.calls('/v1/pair').length).toBeGreaterThan(1); expect(new Set(transport.calls('/v1/pair'))).toEqual(new Set([attempt]));
  expect(result.current.machines[0].state.status).toBe('awaiting-approval');
  mocks.status.mockResolvedValue({ status: 'login-required', loginUrl: null, error: null });
  await act(async () => { await vi.advanceTimersByTimeAsync(3250); });
  expect(transport.calls('/v1/disconnect')).toContain(attempt);
});
it('keeps a working authenticated stream through a control health error but drops it on actual HTTP failure', async () => {
  const peer: NetworkPeer = { id: 'host-1', hostname: 'workstation', online: true, addresses: [] }, transport = new TestTransport();
  transport.operation = async () => JSON.stringify(success(snapshot()));
  mocks.request.mockImplementation(transport.request); mocks.peers.mockResolvedValue([peer]);
  const { result } = renderHook(useShellfox); await act(async () => { await flush(); });
  await act(async () => { await result.current.connect(peer); await flush(); });
  const client = result.current.machines[0].client, connectionId = client.getState().connectionId, terminal = vi.fn(); client.subscribeTerminal(terminal);
  mocks.status.mockResolvedValue({ status: 'error', loginUrl: null, error: 'Control connection unavailable. Reconnecting automatically.' });
  const peerReads = mocks.peers.mock.calls.length;
  await act(async () => { await vi.advanceTimersByTimeAsync(6000); });
  expect(client.getState()).toMatchObject({ status: 'connected', connectionId });
  await act(async () => { transport.events([{ type: 'terminal', event: { type: 'activity', tabId: '20000000-0000-4000-8000-000000000001', busy: true } }]); await flush(); });
  expect(terminal).toHaveBeenCalledOnce(); expect(transport.polls).toHaveLength(2);
  expect(transport.calls('/v1/disconnect')).toHaveLength(0); expect(mocks.peers).toHaveBeenCalledTimes(peerReads);
  await act(async () => { transport.polls.at(-1)?.reject(new Error('HTTP connection lost')); await flush(); });
  expect(client.getState().status).toBe('reconnecting');
});
it('reports real operation failures but does not turn reconnects into sticky warnings', async () => {
  const { result } = renderHook(useShellfox);
  await act(async () => { await flush(); });
  act(() => result.current.report(new RemoteClientError({ code: 'NATIVE_UNAVAILABLE', message: 'Connection failed', retryable: true })));
  expect(result.current.error).toBeNull();
  act(() => result.current.report(new Error('Could not save settings')));
  expect(result.current.error).toBe('Could not save settings');
  await act(async () => { await vi.advanceTimersByTimeAsync(3250); });
  expect(result.current.error).toBe('Could not save settings');
});
