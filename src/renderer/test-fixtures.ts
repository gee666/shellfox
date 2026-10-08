// Typed fixtures. Never imported by the production entry point.
import { vi } from 'vitest';
import type { ChangedEvent, ManagerApi, ManagerSnapshot, SessionDto, SettingsDto, TabDto, TerminalApi, TerminalEvent, TerminalProfileDto } from '../shared/contracts';
import { success } from '../shared/contracts';

export const profiles: TerminalProfileDto[] = [
  { id: 'pwsh', label: 'PowerShell 7', executable: 'C:\\Program Files\\PowerShell\\7\\pwsh.exe', args: [], environment: 'local', distro: null, available: true },
  { id: 'wsl:Ubuntu', label: 'Ubuntu', executable: 'C:\\Windows\\System32\\wsl.exe', args: ['--distribution', 'Ubuntu'], environment: 'wsl', distro: 'Ubuntu', available: true },
];
export const settings: SettingsDto = {
  version: 1, accentColor: '#ec4899', backgroundColor: '#111016', adapterId: 'embedded-pty', shellId: 'pwsh', terminalProfileId: 'pwsh',
  shellExecutable: null, historyPageSize: 20, processRules: [{
    id: '00000000-0000-4000-8000-000000000099', label: 'Pi', enabled: true,
    executableBasenames: ['node.exe'], executablePaths: [], scriptPathSuffixes: ['pi/dist/cli.js'],
  }],
};
type FixtureTab = TabDto & { generation: string; terminalKind: 'embedded' | 'external-legacy'; profileId: string | null; exitCode: number | null };
export function session(index = 1, overrides: Partial<SessionDto> = {}): SessionDto & { tabs: FixtureTab[] } {
  const id = `00000000-0000-4000-8000-${String(index).padStart(12, '0')}`;
  const result: SessionDto = {
    id, title: `Session ${index}`, cwd: 'C:\\projects\\same folder', adapterId: 'embedded-pty', shellId: 'pwsh',
    env: [], terminalLifetime: 'app-owned', shellSurvival: false,
    createdAt: '2026-10-04T10:00:00.000Z', updatedAt: '2026-10-04T10:00:00.000Z', settledAt: null, pinnedAt: null,
    status: 'waiting', activityStatus: 'waiting', counts: { running: 0, waiting: 1, unknown: 0, error: 0, closed: 0, agents: 0 },
    canFocus: true, canAddTab: true, controlReason: null, error: null,
    tabs: [{
      id: `00000000-0000-4000-9000-${String(index).padStart(12, '0')}`, sessionId: id,
      title: 'Shell 1', cwd: 'C:\\projects\\same folder', ordinal: 1, createdAt: '2026-10-04T10:00:00.000Z',
      generation: `00000000-0000-4000-a000-${String(index).padStart(12, '0')}`,
      terminalKind: 'embedded', profileId: 'pwsh', exitCode: null,
      lifecycle: 'open', status: 'waiting', agents: 0, monitoringReason: null, error: null,
    }], ...overrides,
  };
  return { ...result, tabs: result.tabs.map(tab => ({ ...tab,
    generation: tab.generation ?? `00000000-0000-4000-a000-${String(index).padStart(12, '0')}`,
    terminalKind: tab.terminalKind ?? 'embedded', profileId: tab.profileId ?? null, exitCode: tab.exitCode ?? null,
  })) };
}
export function snapshot(sessions: SessionDto[] = [session()]): ManagerSnapshot {
  return {
    revision: 1, sessions, settings: structuredClone(settings),
    probe: { platform: 'win32', arch: 'x64', adapterId: 'embedded-pty', available: true,
      terminalVersion: null, reasons: [],
      capabilities: { createWindow: true, addTab: true, focusWindow: true, processTracking: true, explorerContextMenu: true,
        activateTab: true, splitPane: false, attachExisting: false, closeTerminal: true, commandExitStatus: false,
        embeddedTerminal: true, terminalLifetime: 'app-owned', shellSurvival: false },
      shells: [{ id: 'pwsh', available: true, executable: profiles[0]!.executable, reason: null }],
    },
    cli: { supported: true, installed: false, command: 'shellfox start <path>', reason: null },
    explorer: { supported: true, installed: false, folderItemInstalled: false, backgroundInstalled: false, reason: null },
  };
}
export function mockApi(initial = snapshot()) {
  let current = initial;
  const listeners = new Set<(event: ChangedEvent) => void>();
  const terminalListeners = new Set<(event: TerminalEvent) => void>();
  const unsubscribe = vi.fn(); const terminalUnsubscribe = vi.fn();
  function update(item: SessionDto) {
    current = { ...current, revision: current.revision + 1, sessions: current.sessions.map(s => s.id === item.id ? item : s) };
    listeners.forEach(listener => listener({ revision: current.revision, reason: 'sessions' }));
    return success(item);
  }
  const api = {
    getSnapshot: vi.fn<ManagerApi['getSnapshot']>(async () => success(current)),
    getTerminalProfiles: vi.fn<TerminalApi['getTerminalProfiles']>(async () => success({ profiles, defaultProfileId: 'pwsh', lifetime: 'app-owned', shellSurvival: false })),
    attachTerminal: vi.fn<TerminalApi['attachTerminal']>(async input => {
      const item = current.sessions.find(s => s.tabs.some(t => t.id === input.tabId)) ?? session();
      const tab = item.tabs.find(t => t.id === input.tabId) ?? session().tabs[0]!;
      return success({ tabId: input.tabId, sessionId: item.id, generation: tab.generation ?? session().tabs[0]!.generation, firstSequence: 1, lastSequence: 0, chunks: [], truncated: false,
        state: tab.lifecycle === 'closed' ? 'closed' : 'open', exitCode: tab.exitCode ?? null, cols: 80, rows: 24, lifetime: 'app-owned' });
    }),
    acknowledgeTerminal: vi.fn<TerminalApi['acknowledgeTerminal']>(async () => success({ acknowledged: true })),
    detachTerminal: vi.fn<TerminalApi['detachTerminal']>(async () => success({ detached: true })),
    writeTerminal: vi.fn<TerminalApi['writeTerminal']>(async () => success({ written: true })),
    resizeTerminal: vi.fn<TerminalApi['resizeTerminal']>(async () => success({ resized: true })),
    closeTab: vi.fn<TerminalApi['closeTab']>(async input => {
      const item = current.sessions.find(s => s.tabs.some(t => t.id === input.tabId))!;
      const result = update({ ...item, tabs: item.tabs.map(tab => tab.id === input.tabId ? { ...tab, lifecycle: 'closed', exitCode: 0 } : tab) });
      terminalListeners.forEach(listener => listener({ type: 'exit', tabId: input.tabId, generation: input.generation, exitCode: 0, signal: null, lastSequence: 0 }));
      return result;
    }),
    subscribeTerminal: vi.fn<TerminalApi['subscribeTerminal']>(listener => {
      terminalListeners.add(listener); return () => { terminalListeners.delete(listener); terminalUnsubscribe(); };
    }),
    createSession: vi.fn<ManagerApi['createSession']>(async input => {
      const item = session(current.sessions.length + 1, { cwd: input.cwd, title: input.title ?? 'New session' });
      current = { ...current, revision: current.revision + 1, sessions: [...current.sessions, item] };
      return success(item);
    }),
    activateSession: vi.fn<ManagerApi['activateSession']>(async input => success(current.sessions.find(s => s.id === input.sessionId) ?? current.sessions[0]!)),
    refreshSessionMembership: vi.fn<ManagerApi['refreshSessionMembership']>(async input => success(current.sessions.find(s => s.id === input.sessionId) ?? current.sessions[0]!)),
    prepareSessionRegistration: vi.fn<ManagerApi['prepareSessionRegistration']>(async () => success({ command: 'unused legacy registration', expiresAt: '2026-10-04T12:00:00.000Z', titleMarker: 'SHELLFOX:marker', instructions: 'Legacy only' })),
    addTab: vi.fn<ManagerApi['addTab']>(async input => {
      const item = current.sessions.find(s => s.id === input.sessionId)!;
      const tab = { ...session(100 + item.tabs.length).tabs[0]!, sessionId: item.id, ordinal: item.tabs.length + 1, title: input.title ?? `Shell ${item.tabs.length + 1}`, profileId: input.profileId ?? 'pwsh', cwd: input.cwd ?? item.cwd };
      return update({ ...item, tabs: [...item.tabs, tab] });
    }),
    renameTab: vi.fn<ManagerApi['renameTab']>(async input => {
      const item = current.sessions.find(s => s.id === input.sessionId)!;
      return update({ ...item, tabs: item.tabs.map(tab => tab.id === input.tabId ? { ...tab, title: input.title } : tab) });
    }),
    reorderTabs: vi.fn<ManagerApi['reorderTabs']>(async input => {
      const item = current.sessions.find(s => s.id === input.sessionId)!;
      return update({ ...item, tabs: input.tabIds.map((id, ordinal) => ({ ...item.tabs.find(tab => tab.id === id)!, ordinal })) });
    }),
    focusSession: vi.fn<ManagerApi['focusSession']>(async () => success({ focused: true })),
    renameSession: vi.fn<ManagerApi['renameSession']>(async input => update({ ...current.sessions.find(s => s.id === input.sessionId)!, title: input.title })),
    setSessionPinned: vi.fn<ManagerApi['setSessionPinned']>(async input => update({ ...current.sessions.find(s => s.id === input.sessionId)!, pinnedAt: input.pinned ? '2026-10-05T10:00:00.000Z' : null })),
    settleSession: vi.fn<ManagerApi['settleSession']>(async input => update({ ...current.sessions.find(s => s.id === input.sessionId)!, status: 'settled', settledAt: '2026-10-04T11:00:00.000Z' })),
    unsettleSession: vi.fn<ManagerApi['unsettleSession']>(async () => success(session())),
    deleteSession: vi.fn<ManagerApi['deleteSession']>(async () => success({ deleted: true })),
    clearSessionError: vi.fn<ManagerApi['clearSessionError']>(async () => success(session())),
    retryTab: vi.fn<ManagerApi['retryTab']>(async () => success(session())),
    getHistory: vi.fn<ManagerApi['getHistory']>(async query => success({ items: [], total: 0, page: query.page, pageSize: query.pageSize })),
    saveSettings: vi.fn<ManagerApi['saveSettings']>(async value => {
      current = { ...current, revision: current.revision + 1, settings: value }; return success(value);
    }),
    setExplorerIntegration: vi.fn<ManagerApi['setExplorerIntegration']>(async value => {
      const explorer = { ...current.explorer, installed: value.installed, folderItemInstalled: value.installed, backgroundInstalled: value.installed };
      current = { ...current, revision: current.revision + 1, explorer }; return success(explorer);
    }),
    copyText: vi.fn<ManagerApi['copyText']>(async () => success({ copied: true })),
    readClipboardText: vi.fn<NonNullable<ManagerApi['readClipboardText']>>(async () => success({ text: '' })),
    openSessionFolder: vi.fn<ManagerApi['openSessionFolder']>(async () => success({ opened: true })),
    setSessionEnv: vi.fn<ManagerApi['setSessionEnv']>(async input => update({ ...current.sessions.find(item => item.id === input.sessionId)!, env: structuredClone(input.env) })),
    setCliIntegration: vi.fn<ManagerApi['setCliIntegration']>(async input => {
      const cli = { ...current.cli, installed: input.installed };
      current = { ...current, revision: current.revision + 1, cli };
      listeners.forEach(listener => listener({ revision: current.revision, reason: 'settings' }));
      return success(cli);
    }),
    chooseDirectory: vi.fn<ManagerApi['chooseDirectory']>(async () => success({ cwd: 'C:\\projects\\same folder' })),
    subscribe: vi.fn<ManagerApi['subscribe']>(listener => { listeners.add(listener); return () => { listeners.delete(listener); unsubscribe(); }; }),
  } satisfies ManagerApi;
  return {
    api, unsubscribe, terminalUnsubscribe,
    emitTerminal(event: TerminalEvent) { terminalListeners.forEach(listener => listener(event)); },
    emit(next: ManagerSnapshot, reason: ChangedEvent['reason'] = 'sessions') {
      current = next; listeners.forEach(listener => listener({ revision: next.revision, reason }));
    },
  };
}
export function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
}
