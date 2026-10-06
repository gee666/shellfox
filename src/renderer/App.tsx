import { useEffect, useMemo, useState } from 'react';
import { useStore } from 'zustand';
import type { CSSProperties } from 'react';
import type { ManagerApi, TerminalProfileDto } from '../shared/contracts';
import { getManagerApi, getTerminalApi, request } from './api';
import { createManagerClient } from './store';
import { Icon, Modal, Notifications, useAction, useNotify } from './components';
import { SessionSidebar } from './SessionSidebar';
import { TerminalWorkspace } from './TerminalWorkspace';
import { TerminalRegistry } from './terminal-client';
import { SettingsPanel } from './SettingsPanel';
import { useSidebarWidth } from './sidebar-width';

export function App({ api = getManagerApi() }: { api?: ManagerApi }) {
  if (!api) return <main className="disconnected">Manager connection unavailable</main>;
  return <Notifications><ConnectedApp api={api} /></Notifications>;
}

function ConnectedApp({ api }: { api: ManagerApi }) {
  const client = useMemo(() => createManagerClient(api), [api]);
  const state = useStore(client.store);
  const settingsError = useStore(client.settings.store, state => state.error);
  const terminalApi = useMemo(() => getTerminalApi(api), [api]);
  const registry = useMemo(() => new TerminalRegistry(terminalApi), [terminalApi]);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [profiles, setProfiles] = useState<TerminalProfileDto[]>([]);
  const [defaultProfileId, setDefaultProfileId] = useState<string | null>(null);
  const [width, setWidth] = useSidebarWidth();
  const [dragging, setDragging] = useState(false);
  const action = useAction();
  const notify = useNotify();
  useEffect(() => { client.start(); registry.start(); return () => { client.stop(); registry.stop(); }; }, [client, registry]);
  useEffect(() => {
    let active = true;
    if (terminalApi) void request(() => terminalApi.getTerminalProfiles()).then(result => {
      if (!active) return;
      if (!result.ok) { if (result.error.code !== 'UNSUPPORTED') notify(result.error); return; }
      setProfiles(result.value.profiles); setDefaultProfileId(result.value.defaultProfileId);
    });
    return () => { active = false; };
    // The bridge is stable for the lifetime of this app.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [terminalApi, state.snapshot?.settings.pythonPath]);
  useEffect(() => { if (state.error) notify(state.error); }, [state.error]);
  useEffect(() => { if (settingsError?.toast) notify(settingsError.error); }, [settingsError]);
  const snapshot = state.snapshot;
  useEffect(() => {
    const accent = snapshot?.settings.accentColor;
    if (!accent || !/^#[0-9a-fA-F]{6}$/.test(accent)) return;
    document.documentElement.style.setProperty('--accent', accent);
    document.documentElement.style.setProperty('--accent-rgb', `${parseInt(accent.slice(1, 3), 16)}, ${parseInt(accent.slice(3, 5), 16)}, ${parseInt(accent.slice(5, 7), 16)}`);
  }, [snapshot?.settings.accentColor]);
  const selected = snapshot?.sessions.find(session => session.id === state.selectedId) ?? state.historical;
  const effectiveProfile = snapshot?.settings.terminalProfileId ?? defaultProfileId;
  const available = !!snapshot?.probe.available;
  const canCreate = available && !!snapshot?.probe.capabilities.createWindow && (snapshot.probe.adapterId !== 'embedded-pty' || (!!terminalApi && profiles.some(profile => profile.available && profile.id === effectiveProfile)));
  async function create() {
    // The native folder picker waits for the user. Browsing may take longer than
    // the IPC transport deadline, and a timed-out pick would be silently dropped.
    const directory = await action.run(() => api.chooseDirectory(), { timeoutMs: 0 });
    if (!directory?.ok || !directory.value) return;
    const cwd = directory.value.cwd;
    const title = cwd.replace(/[\\/]+$/, '').split(/[\\/]/).at(-1) || cwd;
    const result = await action.run(() => api.createSession({ cwd, title, requestId: crypto.randomUUID() }));
    if (!result?.ok) { client.refresh(); return; }
    let session = result.value;
    client.accept(session);
    if (!session.tabs.some(tab => tab.lifecycle !== 'closed')) {
      const added = await action.run(() => api.addTab({ sessionId: session.id, ...(effectiveProfile ? { profileId: effectiveProfile } : {}) }));
      if (added?.ok) { session = added.value; client.accept(session); }
    }
    const tab = session.tabs.find(tab => tab.lifecycle !== 'closed');
    if (tab) client.selectTab(session, tab.id);
  }
  function closeSettings() {
    setSettingsOpen(false);
    const tab = selected?.tabs.find(tab => tab.id === state.activeTabIds[selected.id]) ?? selected?.tabs.find(tab => tab.lifecycle !== 'closed');
    if (tab) requestAnimationFrame(() => registry.focus(tab.id));
  }
  return <div className={`app-shell${dragging ? ' resizing' : ''}`} style={{ '--sidebar-width': `${width}px` } as CSSProperties}>
    <aside className="sidebar" aria-label="Session navigation">
      <div className="sidebar-toolbar"><button className="icon-button" aria-label="New session" title={canCreate ? 'New session' : 'Choose an available shell in Settings'} disabled={!canCreate || action.pending} onClick={() => void create()}><Icon name="plus" /></button><button className="icon-button" aria-label="Settings" title="Settings" disabled={!snapshot} onClick={() => setSettingsOpen(true)}><Icon name="settings" /></button></div>
      {snapshot && <SessionSidebar client={client} sessions={snapshot.sessions} selectedId={state.selectedId} busy={state.busyTabIds} invalidation={state.historyStatusVersion} archiveInvalidation={state.archiveVersion} pageSize={snapshot.settings.historyPageSize} />}
    </aside>
    <div className="sidebar-resizer" role="separator" aria-label="Sidebar width" aria-orientation="vertical" aria-valuemin={160} aria-valuemax={Math.max(160, window.innerWidth / 2)} aria-valuenow={width} tabIndex={0}
      onDoubleClick={() => setWidth(220)}
      onKeyDown={event => { if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') { event.preventDefault(); setWidth(width + (event.key === 'ArrowLeft' ? -4 : 4)); } if (event.key === 'Home') setWidth(220); }}
      onPointerDown={event => { if (event.button !== 0) return; event.preventDefault(); event.currentTarget.setPointerCapture(event.pointerId); setDragging(true); }}
      onPointerMove={event => { if (event.currentTarget.hasPointerCapture(event.pointerId)) setWidth(event.clientX); }}
      onPointerUp={event => { if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId); setDragging(false); }}
      onPointerCancel={() => setDragging(false)} onLostPointerCapture={() => setDragging(false)} />
    <main className="workspace" aria-label="Workspace">
      {selected ? <TerminalWorkspace key={selected.id} session={selected} client={client} registry={registry} profiles={profiles} defaultProfileId={effectiveProfile} available={available} /> : <div className="empty-terminal">{state.loading ? 'Loading…' : 'Choose a session or press +'}</div>}
    </main>
    {settingsOpen && snapshot && <Modal title="Settings" onClose={closeSettings}><SettingsPanel client={client} settings={snapshot.settings} explorer={snapshot.explorer} cli={snapshot.cli} profiles={profiles} defaultProfileId={defaultProfileId} /></Modal>}
  </div>;
}
