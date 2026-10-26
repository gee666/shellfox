// @vitest-environment jsdom
import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { RemoteClientError } from './client/protocol';
import { flush } from './client/test-fixtures';
const mocks = vi.hoisted(() => ({
  status: vi.fn(), start: vi.fn(), request: vi.fn(),
  appState: { currentState: 'active', addEventListener: vi.fn() },
}));
vi.mock('react-native', () => ({ AppState: mocks.appState }));
vi.mock('./network', () => ({
  nativeNetwork: { start: mocks.start, request: mocks.request },
  networkStatus: mocks.status,
  discoverHosts: async () => ({ peers: [], discovering: false }),
}));
vi.mock('./preferences', () => ({
  defaultPreferences: { hostname: 'test-phone', remembered: [], fontSize: 14 },
  loadPreferences: async () => ({ hostname: 'test-phone', remembered: [], fontSize: 14 }),
  savePreferences: async () => {},
}));
const { useShellfox } = await import('./useShellfox');
const online = { status: 'online', loginUrl: null, error: null };
beforeEach(() => {
  vi.resetAllMocks(); vi.useFakeTimers();
  mocks.appState.addEventListener.mockReturnValue({ remove: vi.fn() });
  mocks.start.mockResolvedValue(''); mocks.status.mockResolvedValue(online);
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
