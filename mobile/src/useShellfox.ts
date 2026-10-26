import { useCallback, useEffect, useRef, useState } from 'react';
import { AppState } from 'react-native';
import { v4 as uuid } from 'uuid';
import type { ManagerSnapshot } from '../../src/shared/contracts';
import { RemoteClient, type ClientState } from './client/client';
import { nativeNetwork, networkPeers, networkStatus, type NetworkPeer, type NetworkStatus } from './network';
import { defaultPreferences, loadPreferences, savePreferences, type Preferences } from './preferences';

export interface MachineView { peer: NetworkPeer; client: RemoteClient; state: ClientState; snapshot: ManagerSnapshot | null }
export const errorMessage = (error: unknown): string => error instanceof Error ? error.message : 'Something went wrong.';
export function useShellfox() {
  const [preferences, setPreferences] = useState(defaultPreferences);
  const [ready, setReady] = useState(false);
  const [network, setNetwork] = useState<NetworkStatus>({ status: 'starting', loginUrl: null, error: null });
  const [peers, setPeers] = useState<NetworkPeer[]>([]);
  const [machines, setMachines] = useState<MachineView[]>([]);
  const [error, setError] = useState<string | null>(null);
  const clients = useRef(new Map<string, { client: RemoteClient; dispose: () => void }>());
  const current = useRef(defaultPreferences);
  const disconnecting = useRef(new Set<string>());
  const saveQueue = useRef<Promise<void>>(Promise.resolve());
  const loaded = useRef(false);
  const started = useRef(false);
  const foreground = useRef(AppState.currentState === 'active');
  const alive = useRef(true);
  const epoch = useRef(0);
  const startAttempt = useRef(0);
  const pollNow = useRef<() => Promise<void>>(async () => {});

  const report = useCallback((cause: unknown) => { if (alive.current) setError(errorMessage(cause)); }, []);
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
  const refresh = useCallback(async (client: RemoteClient) => {
    const connectionId = client.getState().connectionId;
    const result = await client.request('getSnapshot', {});
    const state = client.getState();
    if (!alive.current || state.status !== 'connected' || state.connectionId !== connectionId) return;
    if (result.ok) setMachines(items => items.map(item => item.client === client && (!item.snapshot || result.value.revision >= item.snapshot.revision) ? { ...item, snapshot: result.value } : item));
    else report(new Error(result.error.message));
  }, [report]);
  const ensureClient = useCallback((peer: NetworkPeer) => {
    const found = clients.current.get(peer.id); if (found) return found.client;
    const client = new RemoteClient(peer.id, nativeNetwork, { createClientId: uuid });
    const unsubState = client.subscribeState(state => {
      if (!alive.current) return;
      setMachines(items => items.map(item => item.client === client ? { ...item, state, snapshot: state.status === 'connected' ? item.snapshot : null } : item));
      if (state.status === 'connected') void refresh(client);
    });
    let refreshing = false, dirty = false;
    const unsubEvents = client.subscribe(event => {
      if (event.type !== 'changed') return;
      dirty = true;
      if (refreshing) return;
      refreshing = true;
      void (async () => { try { while (dirty && alive.current) { dirty = false; await refresh(client); } } finally { refreshing = false; } })();
    });
    clients.current.set(peer.id, { client, dispose: () => { unsubState(); unsubEvents(); client.stop(); } });
    setMachines(items => [...items, { peer, client, state: client.getState(), snapshot: null }]);
    return client;
  }, [refresh]);

  const retryStart = useCallback(async () => {
    if (!alive.current) return;
    const attempt = ++startAttempt.current;
    ++epoch.current; started.current = false;
    setReady(false); setError(null); setNetwork({ status: 'starting', loginUrl: null, error: null });
    try {
      if (!loaded.current) {
        const saved = await loadPreferences();
        if (!alive.current || attempt !== startAttempt.current) return;
        current.current = saved; loaded.current = true; setPreferences(saved);
      }
      await nativeNetwork.start(current.current.hostname);
      if (!alive.current || attempt !== startAttempt.current) return;
      started.current = true; setReady(true); await pollNow.current();
    } catch (cause) {
      if (alive.current && attempt === startAttempt.current) {
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
        if (status.status === 'online') {
          const found = await networkPeers(); if (!valid()) return; setPeers(found);
          setMachines(items => items.map(item => ({ ...item, peer: found.find(peer => peer.id === item.peer.id) ?? { ...item.peer, online: false } })));
          for (const peer of found) {
            if (!peer.online || disconnecting.current.has(peer.id) || !current.current.remembered.includes(peer.id)) continue;
            const client = ensureClient(peer);
            if (client.getState().status !== 'rejected') client.connect();
          }
        } else {
          setPeers([]);
          for (const { client } of clients.current.values()) if (client.getState().status !== 'rejected') client.stop();
        }
      } catch (cause) { if (valid()) report(cause); } finally { if (busy === token) busy = null; }
    };
    pollNow.current = poll;
    void retryStart();
    const timer = setInterval(() => { void poll(); }, 3000);
    const subscription = AppState.addEventListener('change', state => {
      ++epoch.current;
      foreground.current = state === 'active';
      if (foreground.current) void poll();
      else for (const { client } of clients.current.values()) if (client.getState().status !== 'rejected') client.stop();
    });
    return () => {
      alive.current = false; started.current = false; ++epoch.current; ++startAttempt.current;
      clearInterval(timer); subscription.remove();
      for (const entry of clients.current.values()) entry.dispose(); clients.current.clear();
    };
  }, [ensureClient, report, retryStart]);

  const connect = useCallback(async (peer: NetworkPeer) => {
    const token = epoch.current;
    await changePreferences(value => ({ ...value, remembered: [...new Set([...value.remembered, peer.id])] }));
    if (alive.current && foreground.current && started.current && token === epoch.current && !disconnecting.current.has(peer.id)) ensureClient(peer).connect();
  }, [ensureClient, changePreferences]);
  const disconnect = useCallback(async (peerId: string, forget = false) => {
    // Stop immediately, including when another save is ahead of this one.
    disconnecting.current.add(peerId);
    clients.current.get(peerId)?.client.stop();
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
  return { ready, network, peers, machines, preferences, error, setError, report, updatePreferences, connect, disconnect, login, logout, renameDevice, refresh, retryStart };
}
export type ShellfoxModel = ReturnType<typeof useShellfox>;
