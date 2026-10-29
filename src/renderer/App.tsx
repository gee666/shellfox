import { isViewableTab } from '../shared/terminal-view';
import { useEffect, useMemo, useRef, useState } from 'react';
import { useStore } from 'zustand';
import type { CSSProperties } from 'react';
import type { ManagerApi, TerminalProfileDto } from '../shared/contracts';
import { getManagerApi, getTerminalApi, request } from './api';
import { createManagerClient } from './store';
import { Icon, Notifications, useAction, useNotify } from './components';
import { SessionSidebar } from './SessionSidebar';
import { TerminalWorkspace } from './TerminalWorkspace';
import { TerminalLoading, TerminalPlaceholder } from './TerminalPlaceholder';
import { TerminalRegistry } from './terminal-client';
import { SettingsPanel, type SettingsTab } from './SettingsPanel';
import { useSidebarWidth } from './sidebar-width';
import { DirectoryPrompt } from './DirectoryPrompt';
import { UpdateNotice } from './UpdateNotice';
import { CloseConfirmationModal } from './CloseConfirmationModal';
import { applyPalette, derivePalette } from './theme';
import { useVisibleSessions } from './visible-sessions';
import { createRemoteController } from './remote-controller';
import { MachineSidebar } from './MachineSidebar';
import { NewSessionButton } from './NewSessionButton';
import { MachineConnectionPanel } from './MachineConnectionPanel';
import { canCreateSession, type SessionCreationTarget } from './session-creation';
import './remote.css';
import './machine-ui.css';

export function App({ api = getManagerApi() }: { api?: ManagerApi }) {
  if (!api) return <main className="disconnected">Manager connection unavailable</main>;
  return <Notifications><ConnectedApp api={api} /></Notifications>;
}

function ConnectedApp({ api }: { api: ManagerApi }) {
  const root = useRef<HTMLDivElement>(null);
  const navigation = useRef<HTMLDivElement>(null);
  useVisibleSessions(api, root, 'local');
  const client = useMemo(() => createManagerClient(api), [api]);
  const state = useStore(client.store);
  const remoteApi = window.shellfoxRemote;
  const remote = useMemo(() => createRemoteController(remoteApi, api), [remoteApi, api]);
  const machines = useStore(remote.store);
  const [machineId, setMachineId] = useState('local');
  const runtime = machineId === 'local' ? undefined : machines.runtimes[machineId];
  const activeClient = runtime?.client ?? client;
  const activeState = useStore(activeClient.store);
  const settingsError = useStore(client.settings.store, state => state.error);
  const terminalApi = useMemo(() => getTerminalApi(api), [api]);
  const registry = useMemo(() => new TerminalRegistry(terminalApi), [terminalApi]);
  const activeRegistry = runtime?.registry ?? registry;
  const hostSnapshot = machineId === 'local' || runtime ? activeState.snapshot : null;
  const hostApi = activeClient.api;
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsTab, setSettingsTab] = useState<SettingsTab>('General');
  const [directory, setDirectory] = useState<SessionCreationTarget | null>(null);
  const [creationTarget, setCreationTarget] = useState<string | null>(null);
  const priorRequests = useRef(new Set<string>());
  const requestSerial = useRef(0);
  const [profiles, setProfiles] = useState<TerminalProfileDto[]>([]);
  const [defaultProfileId, setDefaultProfileId] = useState<string | null>(null);
  const [width, setWidth] = useSidebarWidth();
  const [dragging, setDragging] = useState(false);
  const action = useAction();
  const notify = useNotify();
  useEffect(() => { client.start(); registry.start(); return () => { client.stop(); registry.stop(); }; }, [client, registry]);
  useEffect(() => { remote.start(); return () => remote.stop(); }, [remote]);
  useEffect(() => { if (machines.error) notify({ ...machines.error, message: machines.error.message.split('\n')[0]!.slice(0, 200) }); }, [machines.error]);
  function reviewRequests() {
    setDirectory(null); setCreationTarget(null); setSettingsTab('Remote access'); setSettingsOpen(true);
    requestAnimationFrame(() => { const section = root.current?.querySelector<HTMLElement>('#client-access-settings'); section?.scrollIntoView?.({ block: 'center' }); section?.focus(); });
  }
  useEffect(() => {
    const pending = machines.state?.pending ?? [];
    const added = pending.filter(peer => !priorRequests.current.has(peer.id));
    priorRequests.current = new Set(pending.map(peer => peer.id));
    if (added.length) notify({ tone: 'info', durationMs: 12_000, key: 'connection-request-' + ++requestSerial.current,
      message: added.length === 1 ? `${added[0]!.hostname} wants to connect` : `${added.length} devices want to connect`, action: { label: 'Review', run: reviewRequests } });
  }, [machines.state?.pending, notify]);
  useEffect(() => {
    let active = true;
    if (terminalApi) void request(() => terminalApi.getTerminalProfiles()).then(result => {
      if (!active) return;
      // Automatic discovery is reflected by unavailable shell controls, not toasts.
      if (!result.ok) return;
      setProfiles(result.value.profiles); setDefaultProfileId(result.value.defaultProfileId);
    });
    return () => { active = false; };
    // The bridge is stable for the lifetime of this app.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [terminalApi, state.snapshot?.settings.pythonPath]);
  // Keep initial connection failures visible, but do not toast failed background
  // refreshes when the existing snapshot remains usable. Mutations use useAction.
  useEffect(() => { if (state.error && !state.snapshot) notify(state.error); }, [state.error, state.snapshot]);
  useEffect(() => { if (settingsError?.toast) notify(settingsError.error); }, [settingsError]);
  const snapshot = state.snapshot;
  useEffect(() => {
    const accent = snapshot?.settings.accentColor;
    if (!accent || !/^#[0-9a-fA-F]{6}$/.test(accent)) return;
    const palette = derivePalette(accent, snapshot.settings.backgroundColor);
    applyPalette(palette);
    registry.setTheme(palette.terminal);
    for (const machine of Object.values(machines.runtimes)) machine.registry.setTheme(palette.terminal);
  }, [snapshot?.settings.accentColor, snapshot?.settings.backgroundColor, registry, machines.runtimes]);
  const selected = hostSnapshot?.sessions.find(session => session.id === activeState.selectedId);
  const hostProfiles = runtime?.profiles ?? profiles;
  const hostDefaultProfile = runtime ? runtime.defaultProfileId : defaultProfileId;
  const effectiveProfile = hostSnapshot?.settings.terminalProfileId ?? hostDefaultProfile;
  const available = !!hostSnapshot?.probe.available;
  const canCreate = canCreateSession(hostSnapshot, hostApi, hostProfiles, hostDefaultProfile);
  const localCanCreate = canCreateSession(snapshot, api, profiles, defaultProfileId);
  const currentPeer = machines.state?.peers.find(peer => peer.id === machineId);
  const requestCount = machines.state?.pending.length ?? 0;
  function chooseMachine(id: string) { setMachineId(id); setDirectory(null); setCreationTarget(null); setSettingsOpen(false); }
  async function prepareSession(id: string) {
    chooseMachine(id); setCreationTarget(id);
    if (id !== 'local' && remoteApi && machines.state?.peers.find(peer => peer.id === id)?.connection !== 'connected') {
      const result = await remote.run(() => remoteApi.connect({ peerId: id }));
      if (!result.ok) setCreationTarget(current => current === id ? null : current);
    }
  }
  useEffect(() => {
    if (!creationTarget || creationTarget !== machineId) return;
    if (!hostSnapshot || !canCreate) return;
    setDirectory({ machineId, name: machineId === 'local' ? 'Local' : currentPeer?.hostname ?? machineId, api: hostApi, client: activeClient,
      profileId: effectiveProfile, platform: hostSnapshot.probe.platform });
    setCreationTarget(null);
  }, [creationTarget, machineId, currentPeer?.connection, currentPeer?.hostname, hostSnapshot, canCreate, hostApi, activeClient, effectiveProfile]);
  async function cancelConnection() {
    setCreationTarget(null);
    if (remoteApi && machineId !== 'local') await remote.run(() => remoteApi.disconnect({ peerId: machineId }));
  }
  async function create(cwd: string): Promise<boolean> {
    const target = directory;
    if (!target) return false;
    const client = target.machineId === 'local' ? target.client : remote.store.getState().runtimes[target.machineId]?.client ?? target.client;
    const title = cwd.replace(/[\\/]+$/, '').split(/[\\/]/).at(-1) || cwd;
    const result = await action.run(() => target.api.createSession({ cwd, title, requestId: crypto.randomUUID() }));
    if (!result?.ok) { client.refresh(); return false; }
    let session = result.value;
    client.accept(session);
    if (!session.tabs.some(tab => isViewableTab(tab))) {
      const added = await action.run(() => target.api.addTab({ sessionId: session.id, ...(target.platform !== 'win32' && target.profileId ? { profileId: target.profileId } : {}) }));
      if (added?.ok) { session = added.value; client.accept(session); }
    }
    const tab = session.tabs.find(tab => isViewableTab(tab));
    if (tab) client.selectTab(session, tab.id);
    return true;
  }
  const [awake, setAwake] = useState(false);
  const awakeVersion = useRef(0);
  useEffect(() => {
    let mounted = true;
    const unsubscribe = api.subscribeKeepAwake?.(next => {
      if (!mounted) return;
      awakeVersion.current++; setAwake(next.active);
    });
    // Subscribe before reading, and never overwrite a newer notification with
    // an older initial query or a delayed mutation response.
    const version = awakeVersion.current;
    if (api.getKeepAwake) void request(() => api.getKeepAwake!()).then(result => {
      if (mounted && result.ok && awakeVersion.current === version) setAwake(result.value.active);
    });
    return () => { mounted = false; awakeVersion.current++; unsubscribe?.(); };
  }, [api]);
  async function toggleAwake() {
    const version = awakeVersion.current;
    const result = await action.run(() => api.setKeepAwake!({ active: !awake }));
    if (result?.ok && awakeVersion.current === version) { awakeVersion.current++; setAwake(result.value.active); }
  }
  function closeSettings() {
    setSettingsOpen(false);
    const tab = selected?.tabs.find(tab => tab.id === activeState.activeTabIds[selected.id]) ?? selected?.tabs.find(tab => isViewableTab(tab));
    if (tab) requestAnimationFrame(() => activeRegistry.focus(tab.id));
  }
  useEffect(() => {
    if (!settingsOpen || directory) return;
    const keydown = (event: KeyboardEvent) => {
      const overlay = Array.from(document.querySelectorAll('[role="dialog"], [role="menu"]')).some(node => !node.closest('[hidden], [inert]'));
      if (event.key === 'Escape' && !event.defaultPrevented && !overlay) {
        event.preventDefault(); closeSettings();
      }
    };
    document.addEventListener('keydown', keydown);
    return () => document.removeEventListener('keydown', keydown);
  });
  return <div ref={root} className={`app-shell${dragging ? ' resizing' : ''}`} style={{ '--sidebar-width': `${width}px` } as CSSProperties}>
    <aside className="sidebar" aria-label="Session navigation">
      <div className="sidebar-toolbar"><NewSessionButton peers={machines.state?.peers ?? []} selectedMachine={machineId} localAvailable={localCanCreate} busy={action.pending} onChoose={id => void prepareSession(id)} />{api.setKeepAwake && <button className={`icon-button${awake ? ' active' : ''}`} aria-label="Keep awake" aria-pressed={awake} data-tip="Keep awake" onClick={() => void toggleAwake()}><Icon name={awake ? 'eye' : 'eye-off'} /></button>}<button className={`icon-button${settingsOpen ? ' active' : ''}`} aria-label="Settings" aria-pressed={settingsOpen} data-tip={requestCount ? `Settings · ${requestCount} access request${requestCount === 1 ? '' : 's'}` : 'Settings'} disabled={!snapshot} onClick={() => settingsOpen ? closeSettings() : requestCount ? reviewRequests() : setSettingsOpen(true)}><Icon name="settings" />{requestCount > 0 && <span className="toolbar-request-count" aria-hidden="true">{requestCount > 9 ? '9+' : requestCount}</span>}</button></div>
      <div ref={navigation} className={remoteApi ? 'machine-navigation' : 'local-navigation'}>
        {snapshot && <section data-machine-id="local"><SessionSidebar onSelect={() => chooseMachine('local')} onSectionSelect={() => chooseMachine('local')} scrollContainer={remoteApi ? navigation : undefined} client={client} sessions={snapshot.sessions} selectedId={machineId === 'local' ? state.selectedId : null} busy={state.busyTabIds} pageSize={snapshot.settings.historyPageSize} /></section>}
        {remoteApi && <MachineSidebar controller={remote} selectedMachine={machineId} onSelect={chooseMachine} scrollContainer={navigation} root={root} pageSize={snapshot?.settings.historyPageSize ?? 20} />}
      </div>
      <UpdateNotice api={api} />
    </aside>
    <div className="sidebar-resizer" role="separator" aria-label="Sidebar width" aria-orientation="vertical" aria-valuemin={160} aria-valuemax={Math.max(160, window.innerWidth / 2)} aria-valuenow={width} tabIndex={0}
      onDoubleClick={() => setWidth(220)}
      onKeyDown={event => { if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') { event.preventDefault(); setWidth(width + (event.key === 'ArrowLeft' ? -4 : 4)); } if (event.key === 'Home') setWidth(220); }}
      onPointerDown={event => { if (event.button !== 0) return; event.preventDefault(); event.currentTarget.setPointerCapture(event.pointerId); setDragging(true); }}
      onPointerMove={event => { if (event.currentTarget.hasPointerCapture(event.pointerId)) setWidth(event.clientX); }}
      onPointerUp={event => { if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId); setDragging(false); }}
      onPointerCancel={() => setDragging(false)} onLostPointerCapture={() => setDragging(false)} />
    <main className="workspace" aria-label="Workspace">
      <div className="workspace-content" data-machine-id={machineId} hidden={settingsOpen} inert={settingsOpen}>
        {selected ? <TerminalWorkspace visible={!settingsOpen} key={`${machineId}:${selected.id}`} session={selected} client={activeClient} registry={activeRegistry} profiles={hostProfiles} defaultProfileId={effectiveProfile} platform={hostSnapshot?.probe.platform} available={available} /> : machineId !== 'local' && !runtime ? <MachineConnectionPanel peer={currentPeer} creating={creationTarget === machineId} onRetry={() => { if (remoteApi) void remote.run(() => remoteApi.connect({ peerId: machineId })); }} onCancel={() => void cancelConnection()} /> : activeState.loading ? <TerminalLoading /> : <TerminalPlaceholder />}
      </div>
      {settingsOpen && snapshot && <SettingsPanel onClose={closeSettings} client={client} settings={snapshot.settings} profiles={profiles} defaultProfileId={defaultProfileId} remote={remote} activeTab={settingsTab} onTabChange={setSettingsTab} />}
    </main>
    {directory && <DirectoryPrompt key={directory.machineId} api={directory.api} remote={directory.machineId !== 'local'} machineName={directory.name} onCreate={create} onClose={() => setDirectory(null)} />}
    <CloseConfirmationModal api={api} />
  </div>;
}
