import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { SessionDto } from '../shared/contracts';
import { parseWslUnc } from '../shared/wsl-path';
import { request } from './api';
import type { ManagerClient } from './store';
import { ContextMenu, Icon, StatusDot, sessionDot, useAction, useNotify } from './components';
import { ShellfoxEnvModal } from './shellfox-env-modal';
import { WindowedSessionList } from './WindowedSessionList';

export function shortenHome(cwd: string) {
  cwd = parseWslUnc(cwd)?.guestPath ?? cwd;
  return cwd.replace(/^[a-z]:[\\/]Users[\\/][^\\/]+(?=[\\/]|$)/i, '~').replace(/^\/home\/[^/]+(?=\/|$)|^\/Users\/[^/]+(?=\/|$)/, '~');
}
export function SessionSidebar({ client, sessions, selectedId, busy, invalidation, archiveInvalidation = 0, pageSize }: {
  client: ManagerClient; sessions: SessionDto[]; selectedId: string | null; busy: Record<string, boolean>; invalidation: number; archiveInvalidation?: number; pageSize: number;
}) {
  const [expanded, setExpanded] = useState(false);
  const [history, setHistory] = useState<SessionDto[]>([]);
  const [total, setTotal] = useState(0);
  const [page, setPage] = useState(1);
  const [historyAttempt, setHistoryAttempt] = useState(0);
  const [menu, setMenu] = useState<{ session: SessionDto; x: number; y: number } | null>(null);
  const [editingEnv, setEditingEnv] = useState<SessionDto | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [name, setName] = useState('');
  const [archiving, setArchiving] = useState<SessionDto | null>(null);
  const [deleting, setDeleting] = useState<SessionDto | null>(null);
  const [morePending, setMorePending] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const historyToken = useRef(0);
  const historyIntent = useRef(false);
  const historyLoading = useRef(false);
  const historyCache = useRef<{
    client: ManagerClient; pageSize: number; statusRefresh: number; archiveInvalidation: number;
    page: number; items: SessionDto[]; total: number;
  } | null>(null);
  const moreFocus = useRef<HTMLElement | null>(null);
  const completedMoreFocus = useRef<HTMLElement | null>(null);
  const action = useAction();
  const notify = useNotify();
  const statusRefresh = expanded ? invalidation : 0;
  useEffect(() => {
    const token = ++historyToken.current;
    const userRequested = historyIntent.current;
    historyIntent.current = false;
    const previous = historyCache.current;
    const cached = expanded && previous?.client === client && previous.pageSize === pageSize
      && previous.statusRefresh === statusRefresh && previous.archiveInvalidation === archiveInvalidation ? previous : null;
    if (!expanded) historyCache.current = null; // Reopening refreshes the loaded range.
    if (cached && page <= cached.page) return () => { historyToken.current++; };
    historyLoading.current = true;
    setMorePending(true);
    // More only appends missing pages. Invalidations refresh the loaded range
    // because archive/restore can shift every later page's offset.
    const firstPage = cached ? cached.page + 1 : 1;
    const pages = expanded ? Array.from({ length: page - firstPage + 1 }, (_, index) => index + firstPage) : [1];
    void Promise.all(pages.map(number => request(() => client.api.getHistory({ search: '', status: 'all', page: number, pageSize: expanded ? pageSize : 1 })))).then(async results => {
      if (token !== historyToken.current) return;
      const first = results[0];
      const shifted = cached && first?.ok && first.value.total !== cached.total;
      if (shifted && results.every(result => result.ok)) {
        // The count changed before its invalidation reached us. Re-read prior
        // offsets, otherwise appending can leave deleted rows or miss inserts.
        const priorPages = Array.from({ length: cached.page }, (_, index) => index + 1);
        const priorResults = await Promise.all(priorPages.map(number => request(() => client.api.getHistory({ search: '', status: 'all', page: number, pageSize }))));
        if (token !== historyToken.current) return;
        results = [...priorResults, ...results];
      }
      historyLoading.current = false;
      completedMoreFocus.current = moreFocus.current;
      moreFocus.current = null;
      setMorePending(false);
      const failed = results.find(result => !result.ok);
      // Count/status refreshes run in the background. Opening history or asking
      // for another page is a user operation and must still report its failure.
      if (failed && !failed.ok) { if (userRequested) notify(failed.error); return; }
      const values = results.flatMap(result => result.ok ? [result.value] : []);
      setTotal(values[0]?.total ?? 0);
      if (!expanded) return; // Only a cheap count request while collapsed.
      const items = [...new Map([...(shifted ? [] : cached?.items ?? []), ...values.flatMap(value => value.items)].map(item => [item.id, item])).values()];
      const nextTotal = values[0]?.total ?? 0;
      const lastPage = Math.max(1, Math.ceil(nextTotal / pageSize));
      historyCache.current = { client, pageSize, statusRefresh, archiveInvalidation, page: Math.min(page, lastPage), items, total: nextTotal };
      setHistory(items);
      client.updateHistory(items);
      if (page > lastPage) setPage(lastPage);
    });
    return () => { historyToken.current++; };
  }, [client, statusRefresh, archiveInvalidation, pageSize, expanded, page, historyAttempt]);
  useLayoutEffect(() => {
    const prior = completedMoreFocus.current;
    completedMoreFocus.current = null;
    if (!prior || (document.activeElement !== document.body && document.activeElement !== prior)) return;
    // A disabled or exhausted More button must not strand keyboard focus on
    // body. Don't steal focus if the user moved elsewhere during the request.
    const lastRow = scrollRef.current?.querySelector('[aria-label="Archived sessions"] [role="listitem"]:last-child');
    const target = prior.isConnected ? prior : lastRow?.querySelector<HTMLElement>('input:not(:disabled), button:not(:disabled)')
      ?? scrollRef.current?.querySelector<HTMLElement>('.archive-toggle');
    target?.focus();
  });
  function more() {
    if (historyLoading.current) return;
    historyLoading.current = true;
    setMorePending(true);
    const active = document.activeElement;
    moreFocus.current = active instanceof HTMLElement && active.classList.contains('history-more') ? active : null;
    historyIntent.current = true;
    // Retry the failed next page instead of skipping it on another click.
    setPage((historyCache.current?.page ?? 0) + 1);
    setHistoryAttempt(value => value + 1);
  }
  /** Selecting a live session with no open terminal (e.g. after a restart) opens one fresh shell.
   * The backend serializes activation and returns the existing shell if one is already live. */
  async function open(session: SessionDto) {
    client.select(session);
    if (session.settledAt || session.status === 'settled' || session.tabs.some(tab => tab.lifecycle !== 'closed')) return;
    if (client.store.getState().openingSessionIds[session.id]) return;
    client.store.setState(state => ({ openingSessionIds: { ...state.openingSessionIds, [session.id]: true } }));
    try {
      const result = await request(() => client.api.activateSession({ sessionId: session.id }));
      if (!result.ok) { notify(result.error); return; }
      if (client.store.getState().selectedId !== session.id) return;
      client.accept(result.value);
      const tab = result.value.tabs.find(item => item.lifecycle !== 'closed');
      if (tab) client.selectTab(result.value, tab.id);
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
  async function archive(session: SessionDto, confirmActive = false) {
    setMenu(null);
    const result = await action.run(() => client.api.settleSession({ sessionId: session.id, confirmActive }));
    if (result?.ok) { setArchiving(null); client.accept(result.value); }
    else if (result && result.error.code === 'SETTLE_CONFIRM_REQUIRED') setArchiving(session);
    client.refresh();
  }
  async function restore(session: SessionDto) {
    setMenu(null);
    const result = await action.run(() => client.api.unsettleSession({ sessionId: session.id }));
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
    return <div key={session.id} className={`session-row${selectedId === session.id ? ' selected' : ''}`}
      onContextMenu={event => { event.preventDefault(); setMenu({ session, x: event.clientX, y: event.clientY }); }}
      onKeyDown={event => {
        if (event.key !== 'ContextMenu' && !(event.shiftKey && event.key === 'F10')) return;
        event.preventDefault();
        const rect = event.currentTarget.getBoundingClientRect();
        setMenu({ session, x: rect.left, y: rect.bottom });
      }}>
      <StatusDot state={sessionDot(session, busy)} />
      {editing ? <div className="session-copy"><input autoFocus aria-label="Session title" value={name} maxLength={160} disabled={action.pending} onChange={event => setName(event.target.value)} onKeyDown={event => { if (event.key === 'Enter') void saveName(session); if (event.key === 'Escape') setRenaming(null); }} /><small title={session.cwd}>{shortenHome(session.cwd)}</small></div>
        : <button className="session-copy" aria-label={`Select session ${session.title}`} aria-pressed={selectedId === session.id} onClick={() => void open(session)}><span className="session-title-line"><span className="session-title" title={session.title} onDoubleClick={() => rename(session)}>{session.title}</span>{session.pinnedAt && !session.settledAt && <span className="session-pin" title="Pinned" aria-label="Pinned"><Icon name="pin" /></span>}</span><small title={session.cwd}>{shortenHome(session.cwd)}</small></button>}
    </div>;
  }
  const live = useMemo(() => sessions.filter(session => !session.settledAt && session.status !== 'settled'), [sessions]);
  const retainedIds = [renaming, menu?.session.id ?? null];
  return <div className="sidebar-scroll" ref={scrollRef}><div role="region" aria-label="Sessions">{live.length ? <WindowedSessionList sessions={live} scrollRef={scrollRef} selectedId={selectedId} retainedIds={retainedIds} renderRow={row} /> : <p className="empty-small">No sessions — press +</p>}</div>
    <button className="archive-toggle" aria-expanded={expanded} onClick={() => { historyIntent.current = !expanded; setExpanded(value => !value); }}><span>Archived · {total}</span><span className={expanded ? 'expanded' : ''}><Icon name="chevron" /></span></button>
    {expanded && <div role="region" aria-label="Archived sessions"><WindowedSessionList sessions={history} scrollRef={scrollRef} selectedId={selectedId} retainedIds={retainedIds} renderRow={row} />{history.length < total && <button className="text-button history-more" disabled={morePending} onClick={() => void more()}>More</button>}</div>}
    {menu && <ContextMenu x={menu.x} y={menu.y} onClose={() => setMenu(null)}>
      {!menu.session.settledAt && <button role="menuitem" disabled={action.pending} onClick={() => void pin(menu.session, !menu.session.pinnedAt)}>{menu.session.pinnedAt ? 'Unpin' : 'Pin to top'}</button>}
      <button role="menuitem" disabled={action.pending} onClick={() => rename(menu.session)}>Rename</button>
      <button role="menuitem" disabled={action.pending} onClick={() => { setEditingEnv(menu.session); setMenu(null); }}>Environment variables…</button>
      <button role="menuitem" disabled={action.pending} onClick={() => { const sessionId = menu.session.id; setMenu(null); void action.run(() => client.api.openSessionFolder({ sessionId })); }}>Open in File Explorer</button>
      <button role="menuitem" disabled={action.pending} onClick={() => { const cwd = menu.session.cwd; setMenu(null); void copyPath(cwd); }}>Copy path</button>
      {(menu.session.status === 'error' || menu.session.error || menu.session.tabs.some(tab => tab.error)) && <button role="menuitem" disabled={action.pending} onClick={() => { const session = menu.session; setMenu(null); void action.run(() => client.api.clearSessionError({ sessionId: session.id })).then(result => { if (result?.ok) client.accept(result.value); }); }}>Clear error</button>}
      {!menu.session.settledAt ? <button role="menuitem" disabled={action.pending} onClick={() => void archive(menu.session)}>Archive</button>
        : menu.session.adapterId === 'embedded-pty' && menu.session.tabs.some(tab => tab.terminalKind === 'embedded') && <button role="menuitem" disabled={action.pending} onClick={() => void restore(menu.session)}>Restore</button>}
      {menu.session.settledAt && <button role="menuitem" disabled={action.pending} onClick={() => { setDeleting(menu.session); setMenu(null); }}>Delete permanently…</button>}
    </ContextMenu>}
    {editingEnv && <ShellfoxEnvModal key={editingEnv.id} client={client} session={editingEnv} onClose={() => setEditingEnv(null)} />}
    {archiving && <div className="inline-confirm archive-confirm" role="dialog" aria-label="Archive session" onKeyDown={event => { if (event.key === 'Escape') setArchiving(null); }}><p>Agents are still running. Archive anyway?</p><div><button autoFocus disabled={action.pending} onClick={() => void archive(archiving, true)}>Archive</button><button onClick={() => setArchiving(null)}>Cancel</button></div></div>}
    {deleting && <div className="inline-confirm archive-confirm" role="dialog" aria-label="Delete session" onKeyDown={event => { if (event.key === 'Escape') setDeleting(null); }}><p>Permanently delete “{deleting.title}”?{deleting.tabs.some(tab => tab.terminalKind === 'embedded' && tab.lifecycle !== 'closed') ? ' Its running terminals will be closed.' : ''} This cannot be undone.</p><div><button autoFocus disabled={action.pending} onClick={() => void remove(deleting)}>Delete</button><button onClick={() => setDeleting(null)}>Cancel</button></div></div>}
  </div>;
}
