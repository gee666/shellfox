import { createStore } from 'zustand/vanilla';
import type { AppError, ManagerApi, ManagerSnapshot, Result, TerminalProfileDto } from '../shared/contracts';
import type { RemoteApi, RemotePeer, RemoteState } from '../shared/remote-contracts';
import { getTerminalApi, request } from './api';
import { createManagerClient, type ManagerClient, type ManagerSelection } from './store';
import { createRemoteManagerApi } from './remote-api';
import { TerminalRegistry } from './terminal-client';

export interface MachineRuntime {
  client: ManagerClient;
  registry: TerminalRegistry;
  profiles: TerminalProfileDto[];
  defaultProfileId: string | null;
}
interface MachinesState {
  supported: boolean;
  state: RemoteState | null;
  runtimes: Record<string, MachineRuntime>;
  error: AppError | null;
}
export interface RemotePeerAction { peerId: string; kind: 'connect' | 'disconnect' | 'forget' }
interface PendingConnection { original: RemotePeer; peer: RemotePeer; acknowledged: boolean }
interface ProfileRefresh {
  client: ManagerClient;
  lastStarted: number;
  inFlight: boolean;
  wanted: boolean;
  loaded: boolean;
  signature: string | null;
  unsubscribe?: () => void;
  errorReported: boolean;
  retired: boolean;
  timer?: ReturnType<typeof setTimeout>;
}
const PROFILE_REFRESH_MS = 5_000;

export function createRemoteController(api: RemoteApi | undefined, local: ManagerApi) {
  const store = createStore<MachinesState>(() => ({ supported: api !== undefined, state: null, runtimes: {}, error: null }));
  const selections = new Map<string, ManagerSelection>();
  const profiles = new Map<string, ProfileRefresh>();
  let active = false;
  let version = 0;
  let lifecycle = 0;
  let currentState: RemoteState | null = null;
  const peerActions = new Map<string, object>();
  const pendingConnections = new Map<string, PendingConnection>();
  let unsubscribe: (() => void) | undefined;

  function retire(id: string, runtime: MachineRuntime) {
    const state = runtime.client.store.getState();
    if (state.snapshot) selections.set(id, { selectedId: state.selectedId, activeTabIds: { ...state.activeTabIds } });
    const refresh = profiles.get(id);
    if (refresh) { refresh.retired = true; clearTimeout(refresh.timer); refresh.unsubscribe?.(); profiles.delete(id); }
    // Disposing a display detaches it. It never closes the remote shell.
    runtime.client.stop(); runtime.registry.dispose();
  }

  function profileSignature(snapshot: ManagerSnapshot | null): string | null {
    return snapshot ? JSON.stringify({ probe: snapshot.probe, pythonPath: snapshot.settings.pythonPath, terminalProfileId: snapshot.settings.terminalProfileId }) : null;
  }
  function refreshProfiles(id: string, changed = false) {
    const runtime = store.getState().runtimes[id];
    if (!active || !runtime) return;
    let refresh = profiles.get(id);
    if (!refresh) {
      refresh = { client: runtime.client, lastStarted: -Infinity, inFlight: false, wanted: false, loaded: false, signature: profileSignature(runtime.client.store.getState().snapshot), errorReported: false, retired: false };
      profiles.set(id, refresh);
      const watched = refresh;
      watched.unsubscribe = runtime.client.store.subscribe(state => {
        const signature = profileSignature(state.snapshot);
        if (signature === null || signature === watched.signature) return;
        const changed = watched.signature !== null;
        watched.signature = signature;
        if (changed) refreshProfiles(id, true);
      });
    }
    if (!changed && (refresh.loaded || refresh.inFlight || refresh.timer || refresh.wanted)) return;
    refresh.wanted = true;
    pumpProfiles(id, refresh);
  }

  function pumpProfiles(id: string, refresh: ProfileRefresh) {
    if (!active || refresh.retired || refresh.inFlight || refresh.timer || !refresh.wanted) return;
    const delay = Math.max(0, refresh.lastStarted + PROFILE_REFRESH_MS - Date.now());
    if (delay) {
      refresh.timer = setTimeout(() => { refresh.timer = undefined; pumpProfiles(id, refresh); }, delay);
      return;
    }
    refresh.wanted = false; refresh.inFlight = true; refresh.lastStarted = Date.now();
    void request(() => refresh.client.api.getTerminalProfiles!(), 0).then(result => {
      if (!active || refresh.retired || store.getState().runtimes[id]?.client !== refresh.client) return;
      if (result.ok) {
        refresh.loaded = true;
        store.setState(current => ({ runtimes: { ...current.runtimes, [id]: {
          ...current.runtimes[id]!, profiles: result.value.profiles, defaultProfileId: result.value.defaultProfileId,
        } } }));
      } else if (!refresh.loaded && !refresh.errorReported) {
        refresh.errorReported = true; store.setState({ error: result.error });
      }
    }).finally(() => {
      refresh.inFlight = false;
      if (!refresh.retired && refresh.wanted) pumpProfiles(id, refresh);
    });
  }

  function displayed(state: RemoteState): RemoteState {
    const peers = new Map(state.peers.map(peer => [peer.id, peer]));
    for (const [id, pending] of pendingConnections) {
      const peer = peers.get(id);
      if (peer) {
        pending.acknowledged ||= peer.connection !== pending.original.connection || peer.remembered !== pending.original.remembered;
        pending.peer = pending.acknowledged ? peer : { ...peer, connection: 'connecting', lastError: null };
      }
      // Discovery can omit a peer while its saved connection is being created.
      peers.set(id, pending.peer);
    }
    return { ...state, peers: [...peers.values()] };
  }
  function publish() { if (currentState) store.setState({ state: displayed(currentState) }); }
  function observe(state: RemoteState) {
    if (!active || !api) return;
    version++; currentState = state;
    const runtimes = { ...store.getState().runtimes };
    // Discovery can go stale during control-plane recovery while peer HTTP stays live.
    const authenticated = state.enabled && state.status !== 'login-required' && state.status !== 'disabled';
    const connected = new Set(state.peers.filter(peer => authenticated && peer.connection === 'connected').map(peer => peer.id));
    for (const [id, runtime] of Object.entries(runtimes)) {
      if (connected.has(id)) continue;
      retire(id, runtime); delete runtimes[id];
    }
    for (const id of connected) {
      if (runtimes[id]) continue;
      const manager = createRemoteManagerApi(api, id, local);
      const runtime: MachineRuntime = { client: createManagerClient(manager, selections.get(id)), registry: new TerminalRegistry(getTerminalApi(manager)), profiles: [], defaultProfileId: null };
      runtimes[id] = runtime;
      runtime.client.start(); runtime.registry.start();
    }
    for (const id of selections.keys()) if (!state.peers.some(peer => peer.id === id && peer.remembered)) selections.delete(id);
    store.setState({ supported: true, state: displayed(state), runtimes, error: null });
    for (const id of connected) refreshProfiles(id);
  }
  async function run(operation: () => Promise<Result<RemoteState>>, options: { initial?: boolean; reportError?: boolean; peerAction?: RemotePeerAction } = {}) {
    const { initial = false, reportError = true, peerAction } = options;
    const epoch = lifecycle, attempt = {};
    if (peerAction) {
      peerActions.set(peerAction.peerId, attempt);
      const peer = currentState?.peers.find(peer => peer.id === peerAction.peerId);
      if (peerAction.kind === 'connect' && peer) pendingConnections.set(peer.id, { original: peer, peer: { ...peer, connection: 'connecting', lastError: null }, acknowledged: false });
      else pendingConnections.delete(peerAction.peerId);
      publish();
    }
    const current = () => active && lifecycle === epoch && (!peerAction || peerActions.get(peerAction.peerId) === attempt);
    const ticket = version;
    try {
      const result = await request(operation);
      if (!current() || initial && version !== ticket) return result;
      if (!result.ok) {
        // Legacy/test backends may expose preload without hosting support.
        if (initial && result.error.code === 'UNSUPPORTED') store.setState({ supported: false, error: null });
        else if (reportError) store.setState({ error: result.error });
      } else if (version === ticket) observe(result.value);
      else if (peerAction && api) {
        // Events may be newer than this action reply, or may concern another peer.
        // Read current state rather than discard the action or overwrite an event.
        const refreshVersion = version;
        const fresh = await request(() => api.getState());
        if (current() && fresh.ok && version === refreshVersion) observe(fresh.value);
      }
      return result;
    } finally {
      if (peerAction && peerActions.get(peerAction.peerId) === attempt) {
        peerActions.delete(peerAction.peerId); pendingConnections.delete(peerAction.peerId);
        if (active && lifecycle === epoch) publish();
      }
    }
  }
  return {
    store, api, run,
    start() {
      if (active || !api) return;
      active = true; lifecycle++;
      unsubscribe = api.subscribe(event => {
        if (event.type === 'state') observe(event.state);
      });
      void run(() => api.getState(), { initial: true });
    },
    stop() {
      active = false; version++; lifecycle++; peerActions.clear(); pendingConnections.clear(); unsubscribe?.(); unsubscribe = undefined;
      for (const [id, runtime] of Object.entries(store.getState().runtimes)) retire(id, runtime);
      store.setState({ runtimes: {} });
    },
  };
}
export type RemoteController = ReturnType<typeof createRemoteController>;
