import { createStore } from 'zustand/vanilla';
import type { AppError, ManagerApi, Result, TerminalProfileDto } from '../shared/contracts';
import type { RemoteApi, RemoteState } from '../shared/remote-contracts';
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
interface ProfileRefresh {
  client: ManagerClient;
  lastStarted: number;
  inFlight: boolean;
  wanted: boolean;
  loaded: boolean;
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
  let unsubscribe: (() => void) | undefined;

  function retire(id: string, runtime: MachineRuntime) {
    const state = runtime.client.store.getState();
    if (state.snapshot) selections.set(id, { selectedId: state.selectedId, activeTabIds: { ...state.activeTabIds } });
    const refresh = profiles.get(id);
    if (refresh) { refresh.retired = true; clearTimeout(refresh.timer); profiles.delete(id); }
    // Disposing a display detaches it. It never closes the remote shell.
    runtime.client.stop(); runtime.registry.dispose();
  }

  function refreshProfiles(id: string) {
    const runtime = store.getState().runtimes[id];
    if (!active || !runtime) return;
    let refresh = profiles.get(id);
    if (!refresh) {
      refresh = { client: runtime.client, lastStarted: -Infinity, inFlight: false, wanted: false, loaded: false, errorReported: false, retired: false };
      profiles.set(id, refresh);
    }
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
    void request(() => refresh.client.api.getTerminalProfiles!()).then(result => {
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

  function observe(state: RemoteState) {
    if (!active || !api) return;
    version++;
    const runtimes = { ...store.getState().runtimes };
    const connected = new Set(state.peers.filter(peer => peer.connection === 'connected' && peer.online).map(peer => peer.id));
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
    store.setState({ supported: true, state, runtimes, error: null });
    for (const id of connected) refreshProfiles(id);
  }
  async function run(operation: () => Promise<Result<RemoteState>>, options: { initial?: boolean; reportError?: boolean } = {}) {
    const { initial = false, reportError = true } = options;
    const ticket = version;
    const result = await request(operation);
    if (!active || initial && version !== ticket) return result;
    if (!result.ok) {
      // Legacy/test backends may expose preload without hosting support.
      if (initial && result.error.code === 'UNSUPPORTED') store.setState({ supported: false, error: null });
      else if (reportError) store.setState({ error: result.error });
    }
    else if (version === ticket) observe(result.value);
    return result;
  }
  return {
    store, api, run,
    start() {
      if (active || !api) return;
      active = true;
      unsubscribe = api.subscribe(event => {
        if (event.type === 'state') observe(event.state);
        else if (event.type === 'changed') refreshProfiles(event.peerId);
      });
      void run(() => api.getState(), { initial: true });
    },
    stop() {
      active = false; version++; unsubscribe?.(); unsubscribe = undefined;
      for (const [id, runtime] of Object.entries(store.getState().runtimes)) retire(id, runtime);
      store.setState({ runtimes: {} });
    },
  };
}
export type RemoteController = ReturnType<typeof createRemoteController>;
