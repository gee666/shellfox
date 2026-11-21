import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode, type RefObject } from 'react';
import { createPortal } from 'react-dom';
import type { SessionDto } from '../shared/contracts';
import { parseWslUnc } from '../shared/wsl-path';
import { request } from './api';
import { isViewableTab } from '../shared/terminal-view';
import type { ManagerClient } from './store';
import { ContextMenu, Icon, StatusDot, sessionDot, useAction, useNotify } from './components';
import { ShellfoxEnvModal } from './shellfox-env-modal';
import { WindowedSessionList } from './WindowedSessionList';
import { sortVisibleSessions } from '../shared/session-order';

export function shortenHome(cwd: string) {
  cwd = parseWslUnc(cwd)?.guestPath ?? cwd;
  return cwd.replace(/^[a-z]:[\\/]Users[\\/][^\\/]+(?=[\\/]|$)/i, '~').replace(/^\/home\/[^/]+(?=\/|$)|^\/Users\/[^/]+(?=\/|$)/, '~');
}
export function SessionSidebar({ client, sessions, selectedId, busy, pageSize, onSelect, label = 'Local', remote = false, actions, onSectionSelect, scrollContainer }: {
  client: ManagerClient; sessions: SessionDto[]; selectedId: string | null; busy: Record<string, boolean>; pageSize: number; onSelect?: () => (() => boolean);
  label?: string; remote?: boolean; actions?: ReactNode; onSectionSelect?: () => void; scrollContainer?: RefObject<HTMLDivElement | null>;
}) {
  const [expanded, setExpanded] = useState(true);
  const [pagination, setPagination] = useState({ pageSize, pages: 1 });
  const [menu, setMenu] = useState<{ session: SessionDto; x: number; y: number } | null>(null);
  const [editingEnv, setEditingEnv] = useState<SessionDto | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [name, setName] = useState('');
  const [deleting, setDeleting] = useState<SessionDto | null>(null);
  const confirmDelete = useRef<HTMLButtonElement>(null);
  // Focus after the context menu's cleanup restores the prior focus.
  useEffect(() => { if (deleting) confirmDelete.current?.focus(); }, [deleting]);
  const ownScrollRef = useRef<HTMLDivElement>(null);
  const scrollRef = scrollContainer ?? ownScrollRef;
  const moreFocus = useRef<HTMLElement | null>(null);
  const action = useAction();
  const notify = useNotify();
  const sorted = useMemo(() => sortVisibleSessions(sessions, busy), [sessions, busy]);
  const pages = pagination.pageSize === pageSize ? pagination.pages : 1;
  const shown = useMemo(() => sorted.slice(0, pages * pageSize), [sorted, pages, pageSize]);
  useLayoutEffect(() => {
    const prior = moreFocus.current;
    moreFocus.current = null;
    if (!prior || prior.isConnected || document.activeElement !== document.body) return;
    // The last page removes Show more. Leave keyboard focus on the last row.
    const rows = ownScrollRef.current?.querySelectorAll('[role="listitem"]');
    rows?.[rows.length - 1]?.querySelector<HTMLElement>('button:not(:disabled)')?.focus();
  });
  function more() {
    moreFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setPagination({ pageSize, pages: pages + 1 });
  }
  /** The backend serializes activation and returns an existing shell if one is live. */
  async function open(session: SessionDto) {
    const stillOnMachine = onSelect?.();
    const current = client.store.getState();
    const opening = current.openingSessionIds[session.id];
    if (opening && current.selectedId === session.id) return;
    client.select(session);
    const { selectionEpoch } = client.store.getState();
    const hasOpenTabs = session.tabs.some(tab => isViewableTab(tab));
    if (opening || (hasOpenTabs && session.adapterId !== 'embedded-pty')) return;
    client.store.setState(state => ({ openingSessionIds: { ...state.openingSessionIds, [session.id]: true } }));
    try {
      const result = await request(() => client.api.activateSession({ sessionId: session.id }));
      const latest = client.store.getState();
      if (latest.selectedId !== session.id || latest.selectionEpoch !== selectionEpoch || stillOnMachine?.() === false) return;
      if (!result.ok) { notify(result.error); return; }
      client.accept(result.value);
      if (!hasOpenTabs) {
        const tab = result.value.tabs.find(item => isViewableTab(item));
        if (tab) client.selectTab(result.value, tab.id);
      }
    } finally {
      client.store.setState(state => {
        const openingSessionIds = { ...state.openingSessionIds };
        delete openingSessionIds[session.id];
        return { openingSessionIds };
      });
    }
  }
  async function pin(session: SessionDto, pinned: boolean) {
    setMenu(null);
    const result = await action.run(() => client.api.setSessionPinned({ sessionId: session.id, pinned }));
    if (result?.ok) client.refresh();
  }
  function rename(session: SessionDto) { setMenu(null); setRenaming(session.id); setName(session.title); }
  async function saveName(session: SessionDto) {
    if (!name.trim()) return;
    const result = await action.run(() => client.api.renameSession({ sessionId: session.id, title: name.trim() }));
    if (result?.ok) { setRenaming(null); client.accept(result.value); }
  }
  async function toggleSettled(session: SessionDto) {
    setMenu(null);
    const result = await action.run(() => sessionDot(session, busy).kind === 'settled'
      ? client.api.unsettleSession({ sessionId: session.id })
      : client.api.settleSession({ sessionId: session.id, confirmActive: false }));
    if (result?.ok) client.accept(result.value);
  }
  async function remove(session: SessionDto) {
    const result = await action.run(() => client.api.deleteSession({ sessionId: session.id }));
    if (result?.ok) { setDeleting(null); client.remove(session.id); }
  }
  async function copyPath(cwd: string) {
    const result = await action.run(() => client.api.copyText({ text: cwd }));
    if (result?.ok) notify({ message: 'Path copied', tone: 'info', durationMs: 2000 });
  }
  function row(session: SessionDto) {
    const editing = renaming === session.id;
    const dot = sessionDot(session, busy);
    const settled = dot.kind === 'settled';
    return <div key={session.id} className={`session-row${selectedId === session.id ? ' selected' : ''}${settled ? ' settled' : ''}`}
      onContextMenu={event => { event.preventDefault(); setMenu({ session, x: event.clientX, y: event.clientY }); }}
      onKeyDown={event => {
        if (event.key !== 'ContextMenu' && !(event.shiftKey && event.key === 'F10')) return;
        event.preventDefault();
        const rect = event.currentTarget.getBoundingClientRect();
        setMenu({ session, x: rect.left, y: rect.bottom });
      }}>
      <StatusDot state={dot} />
      {editing ? <div className="session-copy"><input autoFocus aria-label="Session title" value={name} maxLength={160} disabled={action.pending} onChange={event => setName(event.target.value)} onKeyDown={event => { if (event.key === 'Enter') void saveName(session); if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); setRenaming(null); } }} /><small title={session.cwd}>{shortenHome(session.cwd)}</small></div>
        : <button className="session-copy" aria-label={`Select session ${session.title}`} aria-pressed={selectedId === session.id} onClick={() => void open(session)}><span className="session-title-line"><span className="session-title" title={session.title} onDoubleClick={() => rename(session)}>{session.title}</span>{session.pinnedAt && <span className="session-pin" title="Pinned" aria-label="Pinned"><Icon name="pin" /></span>}</span><small title={session.cwd}>{shortenHome(session.cwd)}</small></button>}
      <button className="icon-button session-settle" aria-label={`${settled ? 'Unsettle' : 'Settle'} session ${session.title}`} title={settled ? 'Unsettle' : 'Settle'} disabled={action.pending} onClick={() => void toggleSettled(session)}><Icon name={settled ? 'unsettle' : 'settle'} /></button>
    </div>;
  }
  const retainedIds = [renaming, menu?.session.id ?? null, deleting?.id ?? null];
  const menuSettled = menu && sessionDot(menu.session, busy).kind === 'settled';
  const deleteConfirmation = deleting && <div className="inline-confirm delete-confirm" role="dialog" aria-label="Delete session" onKeyDown={event => { if (event.key === 'Escape') setDeleting(null); }}><p>Permanently delete “{deleting.title}”?{deleting.tabs.some(tab => tab.terminalKind === 'embedded' && tab.lifecycle !== 'closed') ? ' Its running terminals will be closed.' : ''} This cannot be undone.</p><div><button ref={confirmDelete} disabled={action.pending} onClick={() => void remove(deleting)}>Delete</button><button onClick={() => setDeleting(null)}>Cancel</button></div></div>;
  // A short machine section cannot contain a bottom-aligned confirmation.
  const deleteLayer = deleting ? ownScrollRef.current?.closest('.sidebar-content') : null;
  return <div className="sidebar-scroll" ref={ownScrollRef}>
    <div className="machine-section-heading"><button className="session-section-toggle" aria-label={`${label} · ${sessions.length}`} aria-expanded={expanded} onClick={() => { setExpanded(value => !value); onSectionSelect?.(); }}><span className={`section-chevron${expanded ? ' expanded' : ''}`}><Icon name="chevron" /></span>{remote ? <span className="section-summary"><span className="section-label" title={label}>{label}</span><span className="section-count">· {sessions.length}</span></span> : <span className="section-label">{label} · {sessions.length}</span>}</button>{actions}</div>
    {expanded && <div role="region" aria-label={`${label} sessions`}>{shown.length ? <WindowedSessionList sessions={shown} scrollRef={scrollRef} selectedId={selectedId} retainedIds={retainedIds} renderRow={row} /> : <p className="empty-small">No sessions, press +</p>}{shown.length < sessions.length && <button className="text-button session-more" onClick={more}>Show more</button>}</div>}
    {menu && <ContextMenu x={menu.x} y={menu.y} onClose={() => setMenu(null)}>
      <button role="menuitem" disabled={action.pending} onClick={() => void pin(menu.session, !menu.session.pinnedAt)}>{menu.session.pinnedAt ? 'Unpin' : 'Pin to top'}</button>
      <button role="menuitem" disabled={action.pending} onClick={() => rename(menu.session)}>Rename</button>
      <button role="menuitem" disabled={action.pending} onClick={() => { setEditingEnv(menu.session); setMenu(null); }}>Environment variables…</button>
      {!remote && <button role="menuitem" disabled={action.pending} onClick={() => { const sessionId = menu.session.id; setMenu(null); void action.run(() => client.api.openSessionFolder({ sessionId })); }}>Open in File Explorer</button>}
      <button role="menuitem" disabled={action.pending} onClick={() => { const cwd = menu.session.cwd; setMenu(null); void copyPath(cwd); }}>Copy path</button>
      {(menu.session.status === 'error' || menu.session.error || menu.session.tabs.some(tab => tab.error)) && <button role="menuitem" disabled={action.pending} onClick={() => { const session = menu.session; setMenu(null); void action.run(() => client.api.clearSessionError({ sessionId: session.id })).then(result => { if (result?.ok) client.accept(result.value); }); }}>Clear error</button>}
      <button role="menuitem" disabled={action.pending} onClick={() => void toggleSettled(menu.session)}>{menuSettled ? 'Unsettle' : 'Settle'}</button>
      <button role="menuitem" disabled={action.pending} onClick={() => { setDeleting(menu.session); setMenu(null); }}>Delete…</button>
    </ContextMenu>}
    {editingEnv && <ShellfoxEnvModal key={editingEnv.id} client={client} session={editingEnv} onClose={() => setEditingEnv(null)} />}
    {deleteLayer ? createPortal(deleteConfirmation, deleteLayer) : deleteConfirmation}
  </div>;
}
