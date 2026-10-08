import { useEffect, useState } from 'react';
import { useStore } from 'zustand';
import type { SessionDto, TabDto, TerminalProfileDto } from '../shared/contracts';
import type { ManagerClient } from './store';
import type { TerminalRegistry } from './terminal-client';
import { ContextMenu, Icon, useAction } from './components';
import { TerminalTabs } from './TerminalTabs';
import { TerminalViewport } from './TerminalViewport';
import { getTerminalApi } from './api';

export function TerminalWorkspace({ session, client, registry, profiles, defaultProfileId, platform, available }: {
  session: SessionDto; client: ManagerClient; registry: TerminalRegistry; profiles: TerminalProfileDto[]; defaultProfileId: string | null; platform?: string; available: boolean;
}) {
  const chosen = useStore(client.store, state => state.activeTabIds[session.id]);
  const busy = useStore(client.store, state => state.busyTabIds);
  const [closing, setClosing] = useState<TabDto | null>(null);
  const [closedIds, setClosedIds] = useState<string[]>([]);
  const [profileMenu, setProfileMenu] = useState<{ x: number; y: number } | null>(null);
  const action = useAction();
  const tabs = session.tabs.filter(tab => tab.terminalKind !== 'external-legacy' && tab.lifecycle !== 'closed' && !closedIds.includes(tab.id));
  const selected = tabs.find(tab => tab.id === chosen) ?? tabs[0];
  const archived = !!session.settledAt || session.status === 'settled';
  const terminalApi = getTerminalApi(client.api);
  const legacy = session.adapterId !== 'embedded-pty' || !session.tabs.some(tab => tab.terminalKind === 'embedded');
  const canAdd = !archived && available && session.canAddTab && (legacy || (!!terminalApi && profiles.some(profile => profile.available)));
  useEffect(() => { if (selected && !archived) registry.focus(selected.id); }, [registry, selected?.id, archived]);
  async function add(profileId = platform === 'win32' ? undefined : defaultProfileId ?? undefined) {
    setProfileMenu(null);
    if (archived || !available || !session.canAddTab || (!legacy && (!terminalApi || (profileId !== undefined ? !profiles.some(profile => profile.id === profileId && profile.available) : platform !== 'win32')))) return;
    const ids = new Set(session.tabs.map(tab => tab.id));
    const result = await action.run(() => client.api.addTab({ sessionId: session.id, ...(profileId ? { profileId } : {}) }));
    if (result?.ok && client.store.getState().selectedId === session.id) {
      client.accept(result.value);
      const added = result.value.tabs.find(tab => !ids.has(tab.id) && tab.lifecycle !== 'closed');
      if (added) client.selectTab(result.value, added.id);
    }
    client.refresh();
  }
  async function close(tab: TabDto) {
    if (!tab.generation || !terminalApi) return;
    const index = tabs.findIndex(item => item.id === tab.id);
    const next = tabs[index + 1] ?? tabs[index - 1];
    const wasSelected = selected?.id === tab.id;
    const selectionEpoch = client.store.getState().selectionEpoch;
    const result = await action.run(() => terminalApi.closeTab({ tabId: tab.id, generation: tab.generation! }));
    if (result?.ok) {
      setClosing(null); setClosedIds(ids => [...ids, tab.id]);
      if (client.store.getState().selectedId === session.id) {
        client.accept(result.value);
        if (wasSelected && client.store.getState().selectionEpoch === selectionEpoch && next && result.value.tabs.some(item => item.id === next.id && item.lifecycle !== 'closed')) client.selectTab(result.value, next.id);
      }
    }
    client.refresh();
  }
  function requestClose(tab: TabDto) { if (tab.agents > 0) setClosing(tab); else void close(tab); }
  function select(tab: TabDto) { client.selectTab(session, tab.id); registry.focus(tab.id); }
  async function restore() {
    const result = await action.run(() => client.api.unsettleSession({ sessionId: session.id }));
    if (result?.ok) client.accept(result.value);
  }
  return <div className="terminal-workspace">
    <div className="terminal-tab-strip"><TerminalTabs key={session.id} session={session} tabs={tabs} selectedId={selected?.id} client={client} busy={busy} disabled={action.pending} editable={!archived && !legacy}
      canClose={tab => !archived && !!tab.generation && tab.terminalKind === 'embedded' && !!terminalApi} onSelect={select} onClose={requestClose} /><button className="icon-button new-terminal" aria-label="New terminal" title="New terminal. Right-click to choose a shell." disabled={!canAdd || action.pending} onClick={() => void add()} onContextMenu={event => { event.preventDefault(); if (!archived && available && session.canAddTab && profiles.length) setProfileMenu({ x: event.clientX, y: event.clientY }); }}><Icon name="plus" /></button></div>
    {archived ? <div className="empty-terminal"><span>Archived session</span>{!legacy && <button className="text-button" disabled={action.pending} onClick={() => void restore()}>Restore</button>}</div>
      : !selected ? <div className="empty-terminal">No terminals — press +</div>
        : <div className="terminal-panel" role="tabpanel" id={`terminal-panel-${selected.id}`} aria-labelledby={`terminal-tab-${selected.id}`}>
          {!legacy && <TerminalViewport key={selected.id} registry={registry} tabId={selected.id} generation={selected.generation} visible />}
        </div>}
    {profileMenu && <ContextMenu {...profileMenu} onClose={() => setProfileMenu(null)}>{profiles.map(profile => <button role="menuitem" key={profile.id} disabled={!profile.available || action.pending} title={profile.unavailableReason ?? profile.label} onClick={() => void add(profile.id)}>{profile.label}</button>)}</ContextMenu>}
    {closing && <div className="inline-confirm close-confirm" role="dialog" aria-label={`Close terminal ${closing.title}?`} onKeyDown={event => { if (event.key === 'Escape') setClosing(null); }}><p>An agent is running in this tab. Close?</p><div><button autoFocus disabled={action.pending} onClick={() => void close(closing)}>Close</button><button onClick={() => setClosing(null)}>Cancel</button></div></div>}
  </div>;
}
