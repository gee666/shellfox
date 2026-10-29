// @vitest-environment jsdom
import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { snapshot } from '../../src/renderer/test-fixtures';
import { deferred, flush, TestTransport } from './client/test-fixtures';
import type { DiscoveredHosts, NetworkStatus } from './network';
import type { Preferences } from './preferences';
import { useShellfox } from './useShellfox';

const native = vi.hoisted(() => ({
  os: 'android', start: vi.fn(), status: vi.fn<() => Promise<NetworkStatus>>(),
  hosts: vi.fn<() => Promise<DiscoveredHosts>>(), load: vi.fn<() => Promise<Preferences>>(), request: vi.fn(),
}));
vi.mock('react-native', () => ({
  Platform: { get OS() { return native.os; } },
  AppState: { currentState: 'active', addEventListener: () => ({ remove: vi.fn() }) },
}));
vi.mock('./network', () => ({
  nativeNetwork: { start: native.start, request: native.request }, networkStatus: native.status, discoverHosts: native.hosts,
}));
vi.mock('./preferences', () => ({
  defaultPreferences: { hostname: 'phone', fontSize: 14, remembered: [] },
  loadPreferences: native.load, savePreferences: vi.fn(),
}));
const defaults: Preferences = { hostname: 'phone', fontSize: 14, remembered: [] };
const online: NetworkStatus = { status: 'online', loginUrl: null, error: null };
const host = { id: 'saved-host', hostname: 'workstation', online: true, addresses: ['100.64.0.2'] };
async function settle() { await act(async () => { await flush(); }); }

beforeEach(() => {
  vi.resetAllMocks(); vi.useFakeTimers();
  native.load.mockResolvedValue(defaults);
  native.start.mockResolvedValue('');
  native.status.mockResolvedValue(online);
  native.hosts.mockResolvedValue({ peers: [], discovering: false });
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

describe.each(['android', 'ios'])('initial connection loading on %s', os => {
  beforeEach(() => { native.os = os; });

  it('keeps loading through hydration, native startup, discovery, pairing and the first snapshot despite unrelated errors', async () => {
    const preferences = deferred<Preferences>(), starting = deferred<string>();
    const status = deferred<NetworkStatus>(), hosts = deferred<DiscoveredHosts>();
    const pairing = deferred<string>(), firstSnapshot = deferred<string>();
    native.load.mockReturnValueOnce(preferences.promise);
    native.start.mockReturnValueOnce(starting.promise);
    native.status.mockReturnValueOnce(status.promise);
    native.hosts.mockReturnValueOnce(hosts.promise);
    const transport = new TestTransport();
    transport.pair = () => pairing.promise;
    transport.operation = () => firstSnapshot.promise;
    native.request.mockImplementation(transport.request);
    const { result } = renderHook(useShellfox);
    expect(result.current.loading).toBe(true);
    act(() => result.current.report(new Error('unrelated operation failed')));
    expect(result.current.loading).toBe(true);
    expect(native.start).not.toHaveBeenCalled();

    preferences.resolve({ ...defaults, remembered: [host.id] }); await settle();
    expect(result.current.preferences.remembered).toEqual([host.id]);
    expect(result.current.loading).toBe(true);
    expect(native.status).not.toHaveBeenCalled();
    starting.resolve(''); await settle();
    expect(result.current.ready).toBe(true);
    expect(result.current.loading).toBe(true);
    status.resolve(online); await settle();
    expect(result.current.loading).toBe(true);
    expect(result.current.machines).toHaveLength(0);
    hosts.resolve({ peers: [host], discovering: false }); await settle();
    expect(result.current.machines[0].state.status).toBe('connecting');
    expect(result.current.loading).toBe(true);
    pairing.resolve(JSON.stringify({ ok: true, value: { paired: true, terminalControl: 'exclusive-v1' } })); await settle();
    expect(result.current.machines[0].state.status).toBe('connected');
    expect(result.current.loading).toBe(true);
    firstSnapshot.resolve(JSON.stringify({ ok: true, value: snapshot([]) })); await settle();
    expect(result.current.loading).toBe(false);
    expect(result.current.machines[0].snapshot).not.toBeNull();
  });

  it('does not keep the startup loader running after the first snapshot fails', async () => {
    native.load.mockResolvedValueOnce({ ...defaults, remembered: [host.id] });
    native.hosts.mockResolvedValueOnce({ peers: [host], discovering: false });
    const transport = new TestTransport();
    transport.operation = async () => JSON.stringify({ ok: false, error: { code: 'INTERNAL', message: 'Snapshot failed.', retryable: true } });
    native.request.mockImplementation(transport.request);
    const { result } = renderHook(useShellfox); await settle();
    expect(result.current.machines[0].state.status).toBe('connected');
    expect(result.current.error).toBe('Snapshot failed.');
    expect(result.current.loading).toBe(false);
    act(() => result.current.setError(null));
    expect(result.current.loading).toBe(false);
  });

  it('only finishes an unconfigured startup after persisted preferences and discovery resolve', async () => {
    const preferences = deferred<Preferences>(), hosts = deferred<DiscoveredHosts>();
    native.load.mockReturnValueOnce(preferences.promise);
    native.hosts.mockReturnValueOnce(hosts.promise);
    const { result } = renderHook(useShellfox);
    expect(result.current.loading).toBe(true);
    preferences.resolve(defaults); await settle();
    expect(result.current.loading).toBe(true);
    hosts.resolve({ peers: [], discovering: false }); await settle();
    expect(result.current.loading).toBe(false);
    expect(result.current.preferences.remembered).toEqual([]);
  });

  it.each(['preferences', 'network'])('stops loading on %s failure and stays stopped after dismissing the error', async failure => {
    if (failure === 'preferences') native.load.mockRejectedValueOnce(new Error('storage unavailable'));
    else native.start.mockRejectedValueOnce(new Error('network unavailable'));
    const { result } = renderHook(useShellfox); await settle();
    expect(result.current.network.status).toBe('error');
    expect(result.current.loading).toBe(false);
    act(() => result.current.setError(null));
    expect(result.current.loading).toBe(false);
    const retry = deferred<string>();
    native.start.mockReturnValueOnce(retry.promise);
    act(() => { void result.current.retryStart(); }); await settle();
    expect(result.current.loading).toBe(true);
    retry.resolve(''); await settle();
    expect(result.current.loading).toBe(false);
    expect(result.current.network.status).toBe('online');
  });
});
