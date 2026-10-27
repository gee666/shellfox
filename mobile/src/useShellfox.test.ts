// @vitest-environment jsdom
import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { NetworkPeer, NetworkStatus } from './network';
import type { Preferences } from './preferences';
import { snapshotSchema } from '../../src/shared/schemas';
import type { RemoteResponse } from '../../src/shared/remote-contracts';
import { deferred, flush, ids, session as testSession, TestTransport } from './client/test-fixtures';
import { sessionDot, tabDot } from '../../src/shared/terminal-status';

const mocks = vi.hoisted(() => ({
  appState: { currentState: 'active', addEventListener: vi.fn() }, discovering: false,
  start: vi.fn(), stop: vi.fn(), login: vi.fn(), logout: vi.fn(), request: vi.fn(),
  status: vi.fn<() => Promise<NetworkStatus>>(), peers: vi.fn<() => Promise<NetworkPeer[]>>(),
  load: vi.fn<() => Promise<Preferences>>(), save: vi.fn<(_value: Preferences) => Promise<void>>(),
}));
vi.mock('react-native', () => ({ AppState: mocks.appState }));
vi.mock('./network', () => ({
  nativeNetwork: { start: mocks.start, stop: mocks.stop, login: mocks.login, logout: mocks.logout, request: mocks.request },
  networkStatus: mocks.status, discoverHosts: async () => ({ peers: await mocks.peers(), discovering: mocks.discovering }),
}));
vi.mock('./preferences', () => ({
  defaultPreferences: { hostname: 'shellfox-mobile', remembered: [], fontSize: 14 },
  loadPreferences: mocks.load, savePreferences: mocks.save,
}));
const { errorMessage, useShellfox } = await import('./useShellfox');

const defaults: Preferences = { hostname: 'shellfox-mobile', remembered: [], fontSize: 14 };
const host: NetworkPeer = { id: 'host-1', hostname: 'workstation', online: true, addresses: ['100.64.0.2'] };
const online: NetworkStatus = { status: 'online', loginUrl: null, error: null };
const signedOut: NetworkStatus = { status: 'login-required', loginUrl: null, error: null };
const snapshot = snapshotSchema.parse({
  revision: 1, sessions: [],
  settings: { version: 1, pythonPath: null, accentColor: '#abcdef', backgroundColor: '#111016', adapterId: 'embedded-pty', shellId: 'bash', shellExecutable: null, processRules: [], historyPageSize: 10, sessionSettleHours: 24, sessionDeleteHours: 720 },
  probe: { platform: 'linux', arch: 'x64', adapterId: 'embedded-pty', available: true, terminalVersion: null, shells: [], reasons: [], capabilities: { createWindow: false, addTab: true, focusWindow: false, activateTab: false, splitPane: false, attachExisting: false, closeTerminal: true, commandExitStatus: false, processTracking: false, explorerContextMenu: false } },
  explorer: { supported: false, installed: false, folderItemInstalled: false, backgroundInstalled: false, reason: null },
  cli: { supported: false, installed: false, command: 'shellfox', reason: null },
});
let changeState: (state: string) => void;
let removeListener: ReturnType<typeof vi.fn>;
let transport: TestTransport;
async function settle() { await act(async () => { await flush(); }); }
async function foreground(state: string) { await act(async () => { changeState(state); await flush(); }); }

beforeEach(() => {
  vi.resetAllMocks(); vi.useFakeTimers();
  mocks.appState.currentState = 'active'; mocks.discovering = false;
  removeListener = vi.fn();
  mocks.appState.addEventListener.mockImplementation((_event: string, listener: (state: string) => void) => { changeState = listener; return { remove: removeListener }; });
  mocks.load.mockResolvedValue({ ...defaults, remembered: [] }); mocks.save.mockResolvedValue();
  mocks.start.mockResolvedValue(''); mocks.stop.mockResolvedValue(undefined); mocks.login.mockResolvedValue(''); mocks.logout.mockResolvedValue(undefined);
  mocks.status.mockResolvedValue(signedOut); mocks.peers.mockResolvedValue([host]);
  transport = new TestTransport(); transport.operation = async () => JSON.stringify({ ok: true, value: snapshot });
  mocks.request.mockImplementation(transport.request);
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

describe('mobile state lifecycle with mocked native boundary', () => {
  it('loads saved hostname before starting and does not poll before native start resolves', async () => {
    const starting = deferred<string>(); mocks.start.mockReturnValue(starting.promise);
    mocks.load.mockResolvedValue({ ...defaults, hostname: 'my-phone' });
    const { result } = renderHook(useShellfox); await settle();
    expect(mocks.start).toHaveBeenCalledWith('my-phone'); expect(result.current.ready).toBe(false);
    await act(async () => { await vi.advanceTimersByTimeAsync(6000); });
    await foreground('background'); await foreground('active');
    expect(mocks.status).not.toHaveBeenCalled();
    starting.resolve(''); await settle();
    expect(result.current.ready).toBe(true); expect(mocks.status).toHaveBeenCalledOnce();
  });
  it('shows loading until saved preferences and the initial network status finish', async () => {
    const starting = deferred<string>(); mocks.start.mockReturnValue(starting.promise);
    const { result } = renderHook(useShellfox); await settle();
    expect(result.current.loading).toBe(true);
    starting.resolve(''); await settle();
    expect(result.current.loading).toBe(false);
  });
  it('keeps loading while a remembered host pairs and its first snapshot is pending', async () => {
    mocks.load.mockResolvedValue({ ...defaults, remembered: [host.id] }); mocks.status.mockResolvedValue(online);
    const pending = deferred<string>(); transport.operation = () => pending.promise;
    const { result } = renderHook(useShellfox); await settle();
    expect(result.current.machines[0].state.status).toBe('connected');
    expect(result.current.loading).toBe(true);
    pending.resolve(JSON.stringify({ ok: true, value: snapshot })); await settle();
    expect(result.current.loading).toBe(false);
  });
  it('waits for host discovery and polls it promptly instead of showing an empty connection prompt', async () => {
    mocks.status.mockResolvedValue(online); mocks.peers.mockResolvedValue([]); mocks.discovering = true;
    const { result } = renderHook(useShellfox); await settle();
    expect(result.current.loading).toBe(true);
    const calls = mocks.peers.mock.calls.length;
    mocks.discovering = false;
    await act(async () => { await vi.advanceTimersByTimeAsync(250); });
    expect(mocks.peers.mock.calls.length).toBeGreaterThan(calls); expect(result.current.loading).toBe(false);
  });
  it('tracks terminal activity for desktop-equivalent busy dots and clears it on disconnect', async () => {
    mocks.load.mockResolvedValue({ ...defaults, remembered: [host.id] }); mocks.status.mockResolvedValue(online);
    const { result } = renderHook(useShellfox); await settle();
    const tabId = '20000000-0000-4000-8000-000000000001';
    await act(async () => { transport.events([{ type: 'terminal', event: { type: 'activity', tabId, busy: true } }]); await flush(); });
    expect(result.current.machines[0].busyTabIds[tabId]).toBe(true);
    await foreground('background'); expect(result.current.machines[0].busyTabIds).toEqual({});
  });
  it('recovers session and tab busy dots from the replacement event stream after background reconnect', async () => {
    const activeSnapshot = snapshotSchema.parse({ ...snapshot, sessions: [{ ...testSession, status: 'running', tabs: [{
      id: ids.tab, sessionId: ids.session, title: 'agent', cwd: '/work', ordinal: 0, createdAt: testSession.createdAt,
      lifecycle: 'open', status: 'running', agents: 1, monitoringReason: null, error: null,
    }] }] });
    transport.operation = async () => JSON.stringify({ ok: true, value: activeSnapshot });
    mocks.load.mockResolvedValue({ ...defaults, remembered: [host.id] }); mocks.status.mockResolvedValue(online);
    const { result } = renderHook(useShellfox); await settle();
    const activity = async (busy: boolean) => {
      await act(async () => { transport.events([{ type: 'terminal', event: { type: 'activity', tabId: ids.tab, busy } }]); await flush(); });
    };
    const dotKinds = () => {
      const machine = result.current.machines[0], session = machine.snapshot?.sessions[0];
      if (!session) throw new Error('Missing session');
      return [sessionDot(session, machine.busyTabIds).kind, tabDot(session.tabs[0], machine.busyTabIds[ids.tab]).kind];
    };
    await activity(true); expect(dotKinds()).toEqual(['busy', 'busy']);
    await foreground('background'); expect(result.current.machines[0].busyTabIds).toEqual({});
    transport.events([]); await settle(); await foreground('active');
    expect(transport.calls('/v1/pair')).toHaveLength(2);
    const requests = transport.calls('/v1/request').length;
    await activity(true); expect(dotKinds()).toEqual(['busy', 'busy']);
    await activity(false); expect(dotKinds()).toEqual(['running', 'running']);
    await activity(true); expect(dotKinds()).toEqual(['busy', 'busy']);
    expect(transport.calls('/v1/request')).toHaveLength(requests);
  });
  it('does not start native networking if unmounted while loading preferences', async () => {
    const loading = deferred<Preferences>(); mocks.load.mockReturnValue(loading.promise);
    const { unmount } = renderHook(useShellfox); unmount(); loading.resolve(defaults); await settle();
    expect(mocks.start).not.toHaveBeenCalled(); expect(removeListener).toHaveBeenCalledOnce();
  });
  it('reports startup failure and leaves sign-in disabled', async () => {
    mocks.start.mockRejectedValue(new Error('native startup failed'));
    const { result } = renderHook(useShellfox); await settle();
    expect(result.current.ready).toBe(false); expect(result.current.error).toBe('native startup failed');
    expect(result.current.network).toEqual({ status: 'error', loginUrl: null, error: 'native startup failed' });
    await act(async () => { await vi.advanceTimersByTimeAsync(9000); });
    expect(mocks.status).not.toHaveBeenCalled();
  });
  it('deduplicates connected machines, stops in background and reconnects on resume', async () => {
    mocks.load.mockResolvedValue({ ...defaults, remembered: [host.id] }); mocks.status.mockResolvedValue(online);
    const { result, unmount } = renderHook(useShellfox); await settle();
    expect(result.current.machines).toHaveLength(1); expect(transport.calls('/v1/pair')).toHaveLength(1);
    expect(result.current.machines[0].snapshot).toEqual(snapshot);
    await act(async () => { await vi.advanceTimersByTimeAsync(6000); });
    expect(result.current.machines).toHaveLength(1); expect(transport.calls('/v1/pair')).toHaveLength(1);
    await foreground('background'); expect(result.current.machines[0].state.status).toBe('disconnected');
    expect(result.current.machines[0].snapshot).toEqual(snapshot);
    transport.operation = async () => JSON.stringify({ ok: true, value: { ...snapshot, revision: 0 } });
    const polls = mocks.status.mock.calls.length;
    await act(async () => { await vi.advanceTimersByTimeAsync(6000); }); expect(mocks.status).toHaveBeenCalledTimes(polls);
    transport.events([]); await settle(); await foreground('active'); expect(transport.calls('/v1/pair')).toHaveLength(2);
    expect(result.current.machines[0].snapshot?.revision).toBe(0);
    mocks.peers.mockResolvedValue([{ ...host, online: false }]);
    await act(async () => { await vi.advanceTimersByTimeAsync(3250); });
    expect(result.current.machines[0].state.status).toBe('connected');
    expect(result.current.machines[0].snapshot?.revision).toBe(0);
    unmount(); expect(removeListener).toHaveBeenCalledOnce();
    const requests = mocks.status.mock.calls.length;
    await act(async () => { await vi.advanceTimersByTimeAsync(6000); }); expect(mocks.status).toHaveBeenCalledTimes(requests);
  });
  it('does not retry a rejected device without explicit user connect, including after resume', async () => {
    mocks.load.mockResolvedValue({ ...defaults, remembered: [host.id] }); mocks.status.mockResolvedValue(online);
    transport.pair = async () => JSON.stringify({ ok: false, error: { code: 'ACCESS_DENIED', message: 'Device rejected.', retryable: false } });
    const { result } = renderHook(useShellfox); await settle();
    expect(result.current.machines[0].state.status).toBe('rejected');
    await act(async () => { await vi.advanceTimersByTimeAsync(9000); });
    await foreground('background'); await foreground('active');
    expect(transport.calls('/v1/pair')).toHaveLength(1);
    await act(async () => { await result.current.connect(host); await flush(); });
    expect(transport.calls('/v1/pair')).toHaveLength(2);
  });
  it('disconnect removes auto-connect preference and forget removes the machine', async () => {
    mocks.status.mockResolvedValue(online);
    const { result } = renderHook(useShellfox); await settle();
    await act(async () => { await result.current.connect(host); await flush(); });
    await act(async () => { await result.current.connect(host); await flush(); });
    expect(result.current.preferences.remembered).toEqual([host.id]); expect(result.current.machines).toHaveLength(1);
    await act(async () => { await result.current.disconnect(host.id); });
    expect(result.current.preferences.remembered).toEqual([]); expect(result.current.machines[0].state.status).toBe('disconnected');
    await act(async () => { await vi.advanceTimersByTimeAsync(3000); }); expect(transport.calls('/v1/pair')).toHaveLength(1);
    await act(async () => { await result.current.disconnect(host.id, true); }); expect(result.current.machines).toEqual([]);
  });
  it('failed preference writes do not change state or start a connection', async () => {
    const { result } = renderHook(useShellfox); await settle(); mocks.save.mockRejectedValue(new Error('disk full'));
    await act(async () => { await expect(result.current.connect(host)).rejects.toThrow('disk full'); });
    expect(result.current.preferences).toEqual(defaults); expect(transport.calls('/v1/pair')).toHaveLength(0);
  });
  it('browser sign-in passes no auth key and exposes the returned login URL', async () => {
    const { result } = renderHook(useShellfox); await settle();
    const loginUrl = 'https://login.tailscale.com/a/test-only'; mocks.status.mockResolvedValue({ ...signedOut, loginUrl });
    await act(async () => { expect(await result.current.login()).toEqual({ ...signedOut, loginUrl }); });
    expect(mocks.login).toHaveBeenCalledWith(''); expect(result.current.network.loginUrl).toBe(loginUrl);
    expect(mocks.save).not.toHaveBeenCalled();
  });
  it('logout clears remembered machines and restarts the logged-out node', async () => {
    mocks.load.mockResolvedValue({ ...defaults, remembered: [host.id] }); mocks.status.mockResolvedValue(online);
    const { result } = renderHook(useShellfox); await settle(); mocks.status.mockResolvedValue(signedOut);
    await act(async () => { await result.current.logout(); await flush(); });
    expect(result.current.machines).toEqual([]); expect(result.current.peers).toEqual([]); expect(result.current.preferences.remembered).toEqual([]);
    expect(mocks.logout).toHaveBeenCalledOnce(); expect(mocks.start).toHaveBeenLastCalledWith(defaults.hostname);
    expect(mocks.start.mock.invocationCallOrder.at(-1)).toBeGreaterThan(mocks.logout.mock.invocationCallOrder[0]);
    expect(result.current.network.status).toBe('login-required');
  });
  it('renames native device and persists the new name', async () => {
    const { result } = renderHook(useShellfox); await settle();
    await act(async () => { await result.current.renameDevice('phone-qa'); });
    expect(mocks.stop).toHaveBeenCalledOnce(); expect(mocks.start).toHaveBeenLastCalledWith('phone-qa');
    expect(result.current.preferences.hostname).toBe('phone-qa'); expect(mocks.save).toHaveBeenCalledWith({ ...defaults, hostname: 'phone-qa' });
  });
  it('does not reconnect from an in-flight peer discovery after entering background', async () => {
    const discovering = deferred<NetworkPeer[]>(); mocks.peers.mockReturnValue(discovering.promise);
    mocks.load.mockResolvedValue({ ...defaults, remembered: [host.id] }); mocks.status.mockResolvedValue(online);
    renderHook(useShellfox); await settle(); await foreground('background');
    discovering.resolve([host]); await settle(); expect(transport.calls('/v1/pair')).toHaveLength(0);
  });
  it('does not restore stale discovered peers after logout', async () => {
    const discovering = deferred<NetworkPeer[]>(); mocks.peers.mockReturnValue(discovering.promise); mocks.status.mockResolvedValue(online);
    const { result } = renderHook(useShellfox); await settle(); mocks.status.mockResolvedValue(signedOut);
    await act(async () => { await result.current.logout(); });
    discovering.resolve([host]); await settle(); expect(result.current.peers).toEqual([]);
  });
  it('preserves independent preference patches submitted before storage completes', async () => {
    const first = deferred<void>(); mocks.save.mockReturnValueOnce(first.promise).mockResolvedValueOnce();
    const { result } = renderHook(useShellfox); await settle();
    await act(async () => {
      const font = result.current.updatePreferences({ fontSize: 18 });
      const peers = result.current.updatePreferences({ remembered: [host.id] });
      first.resolve(); await Promise.all([font, peers]);
    });
    expect(result.current.preferences).toEqual({ ...defaults, fontSize: 18, remembered: [host.id] });
    expect(mocks.save).toHaveBeenLastCalledWith({ ...defaults, fontSize: 18, remembered: [host.id] });
  });
  it('retries failed initialization without remounting', async () => {
    mocks.start.mockRejectedValueOnce(new Error('temporarily unavailable')).mockResolvedValue('');
    const { result } = renderHook(useShellfox); await settle();
    expect(result.current.ready).toBe(false);
    await act(async () => { await result.current.retryStart(); });
    expect(result.current.ready).toBe(true); expect(result.current.error).toBeNull();
    expect(result.current.network.status).toBe('login-required'); expect(mocks.load).toHaveBeenCalledOnce();
  });
  it('can retry a preferences read failure', async () => {
    mocks.load.mockRejectedValueOnce(new Error('storage unavailable')).mockResolvedValue(defaults);
    const { result } = renderHook(useShellfox); await settle();
    expect(mocks.start).not.toHaveBeenCalled();
    await act(async () => { await result.current.retryStart(); });
    expect(result.current.ready).toBe(true); expect(mocks.load).toHaveBeenCalledTimes(2);
  });
  it('does not poll or pair if native startup completes in background', async () => {
    const starting = deferred<string>(); mocks.start.mockReturnValue(starting.promise); mocks.status.mockResolvedValue(online);
    mocks.load.mockResolvedValue({ ...defaults, remembered: [host.id] });
    const { result } = renderHook(useShellfox); await settle(); await foreground('background');
    starting.resolve(''); await settle();
    expect(result.current.ready).toBe(true); expect(mocks.status).not.toHaveBeenCalled();
    await foreground('active'); expect(transport.calls('/v1/pair')).toHaveLength(1);
  });
  it('ignores a late network status and its error after backgrounding', async () => {
    const status = deferred<NetworkStatus>(); mocks.status.mockReturnValue(status.promise);
    const { result } = renderHook(useShellfox); await settle(); await foreground('background');
    status.reject(new Error('stale poll failure')); await settle();
    expect(result.current.error).toBeNull(); expect(mocks.peers).not.toHaveBeenCalled();
  });
  it('fences an old discovery across rename, while allowing the new poll to run', async () => {
    const old = deferred<NetworkPeer[]>(); mocks.status.mockResolvedValue(online);
    const newHost = { ...host, id: 'host-2', hostname: 'new-host' };
    mocks.peers.mockReturnValueOnce(old.promise).mockResolvedValue([newHost]);
    const { result } = renderHook(useShellfox); await settle();
    await act(async () => { await result.current.renameDevice('renamed'); });
    expect(result.current.peers).toEqual([newHost]);
    old.resolve([host]); await settle(); expect(result.current.peers).toEqual([newHost]);
  });
  it('does not reconnect from a delayed connect save after entering background', async () => {
    const saving = deferred<void>(); mocks.save.mockReturnValue(saving.promise);
    const { result } = renderHook(useShellfox); await settle();
    let connection: Promise<void>;
    act(() => { connection = result.current.connect(host); }); await settle();
    await foreground('background');
    await act(async () => { saving.resolve(); await connection; });
    expect(transport.calls('/v1/pair')).toHaveLength(0);
  });
  it('does not auto-reconnect while a disconnect preference is still saving', async () => {
    mocks.load.mockResolvedValue({ ...defaults, remembered: [host.id] }); mocks.status.mockResolvedValue(online);
    const { result } = renderHook(useShellfox); await settle();
    const saving = deferred<void>(); mocks.save.mockReturnValue(saving.promise);
    let disconnecting: Promise<void>;
    act(() => { disconnecting = result.current.disconnect(host.id); }); await settle();
    await act(async () => { await vi.advanceTimersByTimeAsync(6000); });
    expect(transport.calls('/v1/pair')).toHaveLength(1);
    await act(async () => { saving.resolve(); await disconnecting; });
  });
  it('does not reconnect when disconnect is queued behind an unrelated save', async () => {
    mocks.load.mockResolvedValue({ ...defaults, remembered: [host.id] }); mocks.status.mockResolvedValue(online);
    const { result } = renderHook(useShellfox); await settle();
    const saving = deferred<void>(); mocks.save.mockReturnValueOnce(saving.promise).mockResolvedValue();
    let font = Promise.resolve(), disconnecting = Promise.resolve();
    act(() => { font = result.current.updatePreferences({ fontSize: 18 }); disconnecting = result.current.disconnect(host.id); });
    await settle(); await act(async () => { await vi.advanceTimersByTimeAsync(6000); });
    expect(transport.calls('/v1/pair')).toHaveLength(1);
    await act(async () => { saving.resolve(); await Promise.all([font, disconnecting]); });
    expect(result.current.preferences).toEqual({ ...defaults, fontSize: 18 });
  });
  it('retains both machines from simultaneous connect requests', async () => {
    const { result } = renderHook(useShellfox); await settle();
    await act(async () => { await Promise.all([result.current.connect(host), result.current.connect({ ...host, id: 'host-2' })]); await flush(); });
    expect(result.current.preferences.remembered).toEqual([host.id, 'host-2']); expect(result.current.machines).toHaveLength(2);
  });
  it('recovers the save queue after a failure without retaining the failed patch', async () => {
    const { result } = renderHook(useShellfox); await settle(); mocks.save.mockRejectedValueOnce(new Error('disk full')).mockResolvedValue();
    await act(async () => { await expect(result.current.updatePreferences({ fontSize: 22 })).rejects.toThrow('disk full'); });
    await act(async () => { await result.current.updatePreferences({ hostname: 'recovered' }); });
    expect(result.current.preferences).toEqual({ ...defaults, hostname: 'recovered' });
  });
  it('reports logout failure without claiming the tailnet is signed out', async () => {
    mocks.status.mockResolvedValue(online); mocks.logout.mockRejectedValue(new Error('logout failed'));
    const { result } = renderHook(useShellfox); await settle();
    await act(async () => { await expect(result.current.logout()).rejects.toThrow('logout failed'); });
    expect(result.current.network.status).toBe('online'); expect(result.current.ready).toBe(true);
    expect(mocks.save).not.toHaveBeenCalled();
  });
  it('restarts the native node even if saving logout preferences fails', async () => {
    mocks.status.mockResolvedValue(online);
    const { result } = renderHook(useShellfox); await settle(); mocks.status.mockResolvedValue(signedOut);
    mocks.save.mockRejectedValue(new Error('storage unavailable'));
    await act(async () => { await expect(result.current.logout()).rejects.toThrow('storage unavailable'); });
    expect(result.current.network.status).toBe('login-required'); expect(result.current.ready).toBe(true);
  });
  it('coalesces explicit and pushed refreshes, with one follow-up for changes during a read', async () => {
    mocks.status.mockResolvedValue(online);
    const { result } = renderHook(useShellfox); await settle();
    await act(async () => { await result.current.connect(host); await flush(); });
    const older = deferred<string>(), newer = deferred<string>(), client = result.current.machines[0].client;
    let requests = 0; transport.operation = () => requests++ === 0 ? older.promise : newer.promise;
    const pending: Promise<void>[] = [];
    act(() => { for (let i = 0; i < 20; i++) pending.push(result.current.refresh(client)); });
    await act(async () => { transport.events([{ type: 'changed', event: { revision: 3, reason: 'native' } }]); await flush(); });
    expect(requests).toBe(1); expect(result.current.error).toBeNull();
    await act(async () => { older.resolve(JSON.stringify({ ok: true, value: { ...snapshot, revision: 3 } })); await pending[0]; await flush(); }); expect(requests).toBe(2);
    await act(async () => { newer.resolve(JSON.stringify({ ok: true, value: { ...snapshot, revision: 2 } })); await Promise.all(pending); });
    expect(result.current.machines[0].snapshot?.revision).toBe(3);
  });
  it('ignores a refresh completed by an older connection epoch', async () => {
    mocks.status.mockResolvedValue(online);
    const { result } = renderHook(useShellfox); await settle();
    await act(async () => { await result.current.connect(host); await flush(); });
    const client = result.current.machines[0].client, oldConnection = client.getState().connectionId;
    const stale = deferred<RemoteResponse<'getSnapshot'>>();
    vi.spyOn(client, 'request').mockReturnValueOnce(stale.promise);
    let pending = Promise.resolve(); act(() => { pending = result.current.refresh(client); });
    await act(async () => { client.reconnect(); await flush(); });
    expect(client.getState().connectionId).not.toBe(oldConnection);
    await act(async () => { stale.resolve({ ok: true, value: { ...snapshot, revision: 999 } }); await pending; });
    expect(result.current.machines[0].snapshot?.revision).toBe(1);
  });
  it('does not restore a snapshot after disconnect while a refresh was pending', async () => {
    mocks.status.mockResolvedValue(online);
    const { result } = renderHook(useShellfox); await settle();
    await act(async () => { await result.current.connect(host); await flush(); });
    const client = result.current.machines[0].client;
    const stale = deferred<RemoteResponse<'getSnapshot'>>(); vi.spyOn(client, 'request').mockReturnValueOnce(stale.promise);
    let pending = Promise.resolve(); act(() => { pending = result.current.refresh(client); });
    await act(async () => { await result.current.disconnect(host.id); });
    await act(async () => { stale.resolve({ ok: true, value: snapshot }); await pending; });
    expect(result.current.machines[0].snapshot).toBeNull();
  });
  it('does not restart a node when an in-flight logout completes after unmount', async () => {
    const loggingOut = deferred<void>(); mocks.logout.mockReturnValue(loggingOut.promise);
    const { result, unmount } = renderHook(useShellfox); await settle();
    let pending = Promise.resolve(); act(() => { pending = result.current.logout(); }); await settle();
    unmount(); loggingOut.resolve(); await pending;
    expect(mocks.start).toHaveBeenCalledOnce();
  });
  it('uses a safe fallback for non-Error exceptions', () => {
    expect(errorMessage({ authKey: 'not-for-ui' })).toBe('Something went wrong.');
  });
});
