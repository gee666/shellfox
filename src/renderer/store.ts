import { createStore } from 'zustand/vanilla';
import type { AppError, ManagerApi, ManagerSnapshot, SessionDto } from '../shared/contracts';
import { request } from './api';
import { createSettingsController } from './settings-controller';

export interface ManagerState {
  snapshot: ManagerSnapshot | null;
  loading: boolean;
  error: AppError | null;
  selectedId: string | null;
  historical: SessionDto | null;
  activeTabIds: Record<string, string>;
  historyStatusVersion: number;
  archiveVersion: number;
  selectionEpoch: number;
  busyTabIds: Record<string, boolean>;
  openingSessionIds: Record<string, boolean>;
}

// One subscription per mounted application. Fetches are serialized so an older
// response cannot replace a newer one, and events during a fetch are not lost.
export function createManagerClient(api: ManagerApi) {
  const store = createStore<ManagerState>(() => ({
    snapshot: null, loading: true, error: null,
    selectedId: null, historical: null, activeTabIds: {}, historyStatusVersion: 0, archiveVersion: 0, selectionEpoch: 0, busyTabIds: {}, openingSessionIds: {},
  }));
  let active = false;
  let generation = 0;
  let dirty = false;
  let revisionFloor = 0;
  let fetching = false;
  let unsubscribe: (() => void) | undefined;
  let unsubscribeTerminal: (() => void) | undefined;
  const settings = createSettingsController(api, refresh);

  async function drain(token: number) {
    if (fetching || !active) return;
    fetching = true;
    try {
      while (dirty && active && token === generation) {
        dirty = false;
        const result = await request(() => api.getSnapshot());
        if (!active || token !== generation) return;
        if (!result.ok) {
          store.setState({ error: result.error, loading: false });
          continue;
        }
        if (result.value.revision < revisionFloor) {
          store.setState({ loading: false, error: {
            code: 'INTERNAL', message: 'Received outdated manager state. Refresh to read the latest update.', retryable: true,
          } });
          continue;
        }
        const state = store.getState();
        if (!state.snapshot || result.value.revision >= state.snapshot.revision) {
          const sessions = result.value.sessions;
          settings.observe(result.value.settings, result.value.revision);
          const previous = state.snapshot?.sessions ?? [];
          // Snapshot comparison must stay linear even with thousands of sessions.
          const nextById = new Map(sessions.map(session => [session.id, session]));
          const previousIds = new Set(previous.map(session => session.id));
          const archiveChanged = previous.some(item => {
            const next = nextById.get(item.id);
            return !next || next.settledAt !== item.settledAt;
          }) || sessions.some(item => item.settledAt && !previousIds.has(item.id));
          const selectedId = state.selectedId ?? sessions.find(session => !session.settledAt)?.id ?? null;
          const activeTabIds = { ...state.activeTabIds };
          const busyTabIds = { ...state.busyTabIds };
          for (const session of sessions) {
            const tabs = session.tabs.filter(tab => tab.lifecycle !== 'closed');
            if (!tabs.some(tab => tab.id === activeTabIds[session.id])) {
              if (tabs[0]) activeTabIds[session.id] = tabs[0].id;
              else delete activeTabIds[session.id];
            }
            session.tabs.filter(tab => tab.lifecycle === 'closed').forEach(tab => { delete busyTabIds[tab.id]; });
          }
          store.setState({
            snapshot: result.value, error: null, loading: false, selectedId, activeTabIds, busyTabIds,
            historical: sessions.some(s => s.id === selectedId) ? null : state.historical,
            archiveVersion: state.archiveVersion + (archiveChanged ? 1 : 0),
          });
        }
      }
    } finally {
      fetching = false;
      // A StrictMode remount can occur while the prior request is still pending.
      if (active && dirty) void drain(generation);
    }
  }

  function refresh() {
    dirty = true;
    if (active) void drain(generation);
  }

  return {
    store,
    api,
    settings,
    start() {
      if (active) return;
      active = true;
      generation += 1;
      try {
        unsubscribe = api.subscribe(event => {
          if (!active) return;
          const selection = (event as { selectSessionId?: string }).selectSessionId;
          if (selection) store.setState(state => ({ selectedId: selection, historical: null, selectionEpoch: state.selectionEpoch + 1 }));
          revisionFloor = Math.max(revisionFloor, event.revision);
          if (event.reason === 'history') store.setState(state => ({ archiveVersion: state.archiveVersion + 1 }));
          else if (event.reason === 'native' || event.reason === 'sessions') store.setState(state => ({ historyStatusVersion: state.historyStatusVersion + 1 }));
          refresh();
        });
        unsubscribeTerminal = api.subscribeTerminal?.(event => {
          if (!active) return;
          const activity = event as { type: string; tabId: string; busy?: boolean };
          if (activity.type === 'activity' && typeof activity.busy === 'boolean') {
            store.setState(state => state.busyTabIds[activity.tabId] === activity.busy ? state
              : { busyTabIds: { ...state.busyTabIds, [activity.tabId]: activity.busy! } });
          } else if (activity.type === 'exit') {
            store.setState(state => state.busyTabIds[activity.tabId] === false ? state
              : { busyTabIds: { ...state.busyTabIds, [activity.tabId]: false } });
          }
        });
        refresh();
      } catch {
        store.setState({ loading: false, error: {
          code: 'INTERNAL', message: 'Could not connect to manager updates. Reopen the manager to reconnect.', retryable: true,
        } });
      }
    },
    stop() {
      active = false;
      generation += 1;
      unsubscribe?.();
      unsubscribe = undefined;
      unsubscribeTerminal?.(); unsubscribeTerminal = undefined;
    },
    refresh,
    select(session: SessionDto) {
      store.setState(state => ({ selectedId: session.id, historical: session.settledAt ? session : null, selectionEpoch: state.selectionEpoch + 1 }));
    },
    selectTab(session: SessionDto, tabId: string) {
      if (!session.tabs.some(tab => tab.id === tabId && tab.lifecycle !== 'closed')) return;
      store.setState(state => ({ selectedId: session.id, historical: session.settledAt ? session : null,
        activeTabIds: { ...state.activeTabIds, [session.id]: tabId }, selectionEpoch: state.selectionEpoch + 1 }));
    },
    accept(session: SessionDto) {
      // An action response is useful immediately, but is not a revisioned snapshot.
      // Never splice it into the live list; re-read the authoritative snapshot.
      const state = store.getState();
      const previous = state.snapshot?.sessions.find(item => item.id === session.id) ?? state.historical;
      const archiveChanged = previous?.settledAt !== session.settledAt && (!!previous?.settledAt || !!session.settledAt);
      store.setState({ selectedId: session.id, historical: session.settledAt ? session : null,
        archiveVersion: state.archiveVersion + (archiveChanged ? 1 : 0), historyStatusVersion: state.historyStatusVersion + 1 });
      refresh();
    },
    remove(sessionId: string) {
      // The session no longer exists anywhere. Drop any selection of it and re-read the authoritative snapshot.
      store.setState(state => {
        const activeTabIds = { ...state.activeTabIds };
        delete activeTabIds[sessionId];
        return { activeTabIds, archiveVersion: state.archiveVersion + 1, historyStatusVersion: state.historyStatusVersion + 1,
          ...(state.selectedId === sessionId ? { selectedId: state.snapshot?.sessions.find(item => item.id !== sessionId && !item.settledAt)?.id ?? null, historical: null, selectionEpoch: state.selectionEpoch + 1 } : {}) };
      });
      refresh();
    },
    updateHistory(items: SessionDto[]) {
      const state = store.getState();
      if (!state.historical) return;
      const selected = items.find(item => item.id === state.selectedId);
      // A history page is partial. Keep a selection from another page.
      if (selected) store.setState({ historical: selected });
    },
  };
}

export type ManagerClient = ReturnType<typeof createManagerClient>;
