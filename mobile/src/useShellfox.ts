import { useCallback, useEffect, useRef, useState } from 'react';
import { AppState } from 'react-native';
import { v4 as uuid } from 'uuid';
import type { ManagerSnapshot } from '../../src/shared/contracts';
import { RemoteClient, type ClientState } from './client/client';
import { RemoteClientError } from './client/protocol';
import { isConnectionFailure } from './connection-status';
import { nativeNetwork, discoverHosts, networkStatus, type NetworkPeer, type NetworkStatus } from './network';
import { defaultPreferences, loadPreferences, savePreferences, type Preferences } from './preferences';

export interface MachineView { peer: NetworkPeer; client: RemoteClient; state: ClientState; snapshot: ManagerSnapshot | null; snapshotConnectionId: string | null; busyTabIds: Record<string, boolean> }
interface SnapshotRefresh { connectionId: string | null; promise: Promise<void>; next?: SnapshotRefresh }
export const errorMessage = (error: unknown): string => error instanceof Error ? error.message : 'Something went wrong.';
export function useShellfox() {
  const [preferences, setPreferences] = useState(defaultPreferences);
  const [ready, setReady] = useState(false);
  const [preferencesLoaded, setPreferencesLoaded] = useState(false);
  const [discovering, setDiscovering] = useState(true);
  const fastPoll = useRef(true);
  const [network, setNetwork] = useState<NetworkStatus>({ status: 'starting', loginUrl: null, error: null });
  const [peers, setPeers] = useState<NetworkPeer[]>([]);
  const [machines, setMachines] = useState<MachineView[]>([]);
  const [snapshotRequests, setSnapshotRequests] = useState(new Map<RemoteClient, string | null>());
  const [error, setError] = useState<string | null>(null);
  const clients = useRef(new Map<string, { client: RemoteClient; dispose: () => void }>());
  const current = useRef(defaultPreferences);
  const disconnecting = useRef(new Set<string>());
  const cancelled = useRef(new Set<string>());
  const saveQueue = useRef<Promise<void>>(Promise.resolve());
  const loaded = useRef(false);
  const started = useRef(false);
  const foreground = useRef(AppState.currentState === 'active');
  const alive = useRef(true);
  const epoch = useRef(0);
  const startAttempt = useRef(0);
  const pollNow = useRef<() => Promise<void>>(async () => {});
  const refreshes = useRef(new WeakMap<RemoteClient, SnapshotRefresh>());

  const report = useCallback((cause: unknown) => {
    if (cause instanceof RemoteClientError && isConnectionFailure(cause.detail)) return;
    if (alive.current) setError(errorMessage(cause));
  }, []);
  // Serialize patches, but let polling see the pending choice immediately. Roll
  // back a failed save and keep the UI on the last successfully persisted value.
  const changePreferences = useCallback((change: (value: Preferences) => Preferences) => {
    const work = saveQueue.current.then(async () => {
      const previous = current.current;
      const next = change(previous);
      current.current = next;
      try { await savePreferences(next); }
      catch (cause) { current.current = previous; throw cause; }
      if (alive.current) setPreferences(next);
    });
    saveQueue.current = work.catch(() => {});
    return work;
  }, []);
  const updatePreferences = useCallback((patch: Partial<Preferences>) => changePreferences(value => ({ ...value, ...patch })), [changePreferences]);
  const readSnapshot = useCallback(async (client: RemoteClient) => {
    const connectionId = client.getState().connectionId;
    setSnapshotRequests(requests => new Map(requests).set(client, connectionId));
    const result = await client.request('getSnapshot', {}).finally(() => {
      if (alive.current) setSnapshotRequests(requests => {
        if (requests.get(client) !== connectionId) return requests;
        const next = new Map(requests);
        next.delete(client);
        return next;
      });
    });
    const state = client.getState();
    if (!alive.current || !foreground.current || state.status !== 'connected' || state.connectionId !== connectionId) return;
    if (result.ok) {
      const tabIds = new Set(result.value.sessions.flatMap(session => session.tabs.map(tab => tab.id)));
      setMachines(items => items.map(item => item.client === client && (!item.snapshot || item.snapshotConnectionId !== connectionId || result.value.revision >= item.snapshot.revision)
        ? { ...item, snapshot: result.value, snapshotConnectionId: connectionId, busyTabIds: Object.fromEntries(Object.entries(item.busyTabIds).filter(([id, busy]) => busy && tabIds.has(id))) }
        : item));
    }
    else if (!isConnectionFailure(result.error)) report(new RemoteClientError(result.error));
  }, [report]);
  const refresh = useCallback((client: RemoteClient): Promise<void> => {
    const connectionId = client.getState().connectionId;
    const previous = refreshes.current.get(client);
    const pending = previous?.connectionId === connectionId ? previous : undefined;
    if (pending?.next) return pending.next.promise;
    const task: SnapshotRefresh = { connectionId, promise: Promise.resolve() };
    if (pending) pending.next = task;
    else refreshes.current.set(client, task);
    // Callers wait for their read, not for the host to stop changing indefinitely.
    const read = async () => {
      if (!alive.current || !foreground.current || client.getState().status !== 'connected' || client.getState().connectionId !== connectionId) return;
      refreshes.current.set(client, task);
      await readSnapshot(client);
    };
    task.promise = (pending ? pending.promise.then(read) : read())
      .finally(() => { if (refreshes.current.get(client) === task && !task.next) refreshes.current.delete(client); });
    return task.promise;
  }, [readSnapshot]);
  const ensureClient = useCallback((peer: NetworkPeer) => {
    const found = clients.current.get(peer.id); if (found) return found.client;
    const client = new RemoteClient(peer.id, nativeNetwork, { createClientId: uuid });
    const unsubState = client.subscribeState(state => {
      if (!alive.current) return;
      const denied = state.status === 'rejected' || state.status === 'awaiting-approval';
      setMachines(items => items.map(item => item.client === client ? {
        ...item, state, snapshot: denied ? null : item.snapshot,
        snapshotConnectionId: denied ? null : item.snapshotConnectionId,
        busyTabIds: state.status === 'connected' ? item.busyTabIds : {},
      } : item));
      if (state.status === 'connected') void refresh(client);
    });
    const unsubEvents = client.subscribe(event => {
      if (event.type === 'terminal') {
        const terminal = event.event;
        if (terminal.type === 'activity' || terminal.type === 'exit') {
          setMachines(items => items.map(item => {
            if (item.client !== client) return item;
            const busyTabIds = { ...item.busyTabIds };
            if (terminal.type === 'activity' && terminal.busy) busyTabIds[terminal.tabId] = true;
            else delete busyTabIds[terminal.tabId];
            return { ...item, busyTabIds };
          }));
        }
        return;
      }
      void refresh(client);
    });
    clients.current.set(peer.id, { client, dispose: () => { unsubState(); unsubEvents(); client.stop(); } });
    setMachines(items => [...items, { peer, client, state: client.getState(), snapshot: null, snapshotConnectionId: null, busyTabIds: {} }]);
    return client;
  }, [refresh]);

  const retryStart = useCallback(async () => {
    if (!alive.current) return;
    const attempt = ++startAttempt.current;
    ++epoch.current; started.current = false;
    fastPoll.current = true; setDiscovering(true);
    setReady(false); setError(null); setNetwork({ status: 'starting', loginUrl: null, error: null });
    try {
      if (!loaded.current) {
        const saved = await loadPreferences();
        if (!alive.current || attempt !== startAttempt.current) return;
        current.current = saved; loaded.current = true; setPreferences(saved); setPreferencesLoaded(true);
      }
      await nativeNetwork.start(current.current.hostname);
      if (!alive.current || attempt !== startAttempt.current) return;
      started.current = true; setReady(true); await pollNow.current();
    } catch (cause) {
      if (alive.current && attempt === startAttempt.current) {
        fastPoll.current = false; setDiscovering(false);
        setNetwork({ status: 'error', loginUrl: null, error: errorMessage(cause) }); report(cause);
      }
    }
  }, [report]);

  useEffect(() => {
    alive.current = true;
    foreground.current = AppState.currentState === 'active';
    let busy: number | null = null;
    const poll = async () => {
      if (!started.current || busy === epoch.current || !foreground.current || !alive.current) return;
      const token = epoch.current;
      busy = token;
      const valid = () => alive.current && foreground.current && started.current && token === epoch.current;
      try {
        const status = await networkStatus(); if (!valid()) return; setNetwork(status);
        fastPoll.current = status.status === 'starting';
        if (status.status === 'online') {
          const discovery = await discoverHosts(); if (!valid()) return;
          const found = discovery.peers; setPeers(found); setDiscovering(discovery.discovering);
          fastPoll.current = discovery.discovering;
          setMachines(items => items.map(item => ({ ...item, peer: found.find(peer => peer.id === item.peer.id) ?? { ...item.peer, online: false } })));
          for (const { client } of clients.current.values()) {
            const status = client.getState().status;
            if (status !== 'rejected' && status !== 'disconnected' && status !== 'connected' && !found.some(peer => peer.id === client.peerId && peer.online)) client.suspend();
          }
          for (const peer of found) {
            if (!peer.online || disconnecting.current.has(peer.id) || cancelled.current.has(peer.id) || !current.current.remembered.includes(peer.id)) continue;
            const client = ensureClient(peer);
            if (client.getState().status !== 'rejected') client.connect();
          }
        } else {
          setDiscovering(status.status === 'starting'); setPeers([]);
          for (const { client } of clients.current.values()) {
            if (status.status === 'login-required' || status.status === 'disabled') client.stop();
            else if (client.getState().status !== 'connected') client.suspend();
          }
        }
      } catch (cause) {
        if (valid()) {
          fastPoll.current = false; setDiscovering(false);
          setNetwork({ status: 'error', loginUrl: null, error: errorMessage(cause) });
        }
      } finally { if (busy === token) busy = null; }
    };
    pollNow.current = poll;
    void retryStart();
    let timer: ReturnType<typeof setTimeout>;
    const tick = async () => {
      await poll();
      if (alive.current) timer = setTimeout(() => { void tick(); }, fastPoll.current ? 250 : 3000);
    };
    timer = setTimeout(() => { void tick(); }, 250);
    const subscription = AppState.addEventListener('change', state => {
      ++epoch.current;
      foreground.current = state === 'active';
      if (foreground.current) void poll();
      else for (const { client } of clients.current.values()) client.suspend();
    });
    return () => {
      alive.current = false; started.current = false; ++epoch.current; ++startAttempt.current;
      clearTimeout(timer); subscription.remove();
      for (const entry of clients.current.values()) entry.dispose(); clients.current.clear();
    };
  }, [ensureClient, report, retryStart]);

  const connect = useCallback(async (peer: NetworkPeer) => {
    const token = epoch.current;
    await changePreferences(value => ({ ...value, remembered: [...new Set([...value.remembered, peer.id])] }));
    cancelled.current.delete(peer.id);
    if (alive.current && foreground.current && started.current && token === epoch.current && !disconnecting.current.has(peer.id)) ensureClient(peer).connect();
  }, [ensureClient, changePreferences]);
  const disconnect = useCallback(async (peerId: string, forget = false) => {
    // Stop immediately, including when another save is ahead of this one.
    disconnecting.current.add(peerId); cancelled.current.add(peerId);
    clients.current.get(peerId)?.client.stop();
    setMachines(items => items.map(item => item.peer.id === peerId ? { ...item, snapshot: null, snapshotConnectionId: null, busyTabIds: {} } : item));
    try {
      await changePreferences(value => ({ ...value, remembered: value.remembered.filter(id => id !== peerId) }));
      if (forget) { clients.current.get(peerId)?.dispose(); clients.current.delete(peerId); if (alive.current) setMachines(items => items.filter(item => item.peer.id !== peerId)); }
    } finally { disconnecting.current.delete(peerId); }
  }, [changePreferences]);
  const login = useCallback(async (authKey = '') => {
    const token = epoch.current;
    await nativeNetwork.login(authKey);
    const status = await networkStatus();
    if (alive.current && token === epoch.current) setNetwork(status);
    return status;
  }, []);
  const logout = useCallback(async () => {
    ++epoch.current; ++startAttempt.current; started.current = false; setReady(false);
    for (const entry of clients.current.values()) entry.dispose(); clients.current.clear(); setMachines([]); setPeers([]);
    try {
      await nativeNetwork.logout();
      await changePreferences(value => ({ ...value, remembered: [] }));
    } finally {
      // Logout stops the native node. Restart even after a persistence failure, and
      // read the real status rather than claiming sign-out succeeded on an error.
      await retryStart();
    }
  }, [changePreferences, retryStart]);
  const renameDevice = useCallback(async (hostname: string) => {
    ++epoch.current; ++startAttempt.current; started.current = false; setReady(false);
    for (const { client } of clients.current.values()) if (client.getState().status !== 'rejected') client.stop();
    try {
      await nativeNetwork.stop();
      await updatePreferences({ hostname });
    } finally { await retryStart(); }
  }, [updatePreferences, retryStart]);
  const loading = network.status !== 'error' && (!ready || network.status === 'starting'
    || network.status === 'online' && (discovering && machines.length === 0
      || machines.length > 0 && machines.every(machine => !machine.snapshot && (
        machine.state.status === 'connecting' || machine.state.status === 'reconnecting'
        || machine.state.status === 'connected' && snapshotRequests.get(machine.client) === machine.state.connectionId))));
  return { ready, preferencesLoaded, loading, network, peers, machines, preferences, error, setError, report, updatePreferences, connect, disconnect, login, logout, renameDevice, refresh, retryStart };
}
export type ShellfoxModel = ReturnType<typeof useShellfox>;
