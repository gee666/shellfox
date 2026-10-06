import { useEffect, useRef, useState } from 'react';
import type { SessionDto } from '../shared/contracts';
import { request } from './api';
import type { ManagerClient } from './store';
import { ContextMenu, Icon, StatusDot, sessionDot, useAction, useNotify } from './components';
import { ShellfoxEnvModal } from './shellfox-env-modal';

export function shortenHome(cwd: string) {
  return cwd.replace(/^[a-z]:[\\/]Users[\\/][^\\/]+(?=[\\/]|$)/i, '~').replace(/^\/home\/[^/]+(?=\/|$)|^\/Users\/[^/]+(?=\/|$)/, '~');
}
export function SessionSidebar({ client, sessions, selectedId, busy, invalidation, archiveInvalidation = 0, pageSize }: {
  client: ManagerClient; sessions: SessionDto[]; selectedId: string | null; busy: Record<string, boolean>; invalidation: number; archiveInvalidation?: number; pageSize: number;
}) {
  const [expanded, setExpanded] = useState(false);
  const [history, setHistory] = useState<SessionDto[]>([]);
  const [total, setTotal] = useState(0);
  const [page, setPage] = useState(1);
  const [menu, setMenu] = useState<{ session: SessionDto; x: number; y: number } | null>(null);
  const [editingEnv, setEditingEnv] = useState<SessionDto | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [name, setName] = useState('');
  const [archiving, setArchiving] = useState<SessionDto | null>(null);
  const [morePending, setMorePending] = useState(false);
  const historyToken = useRef(0);
  const action = useAction();
  const notify = useNotify();
  const statusRefresh = expanded ? invalidation : 0;
  useEffect(() => {
    const token = ++historyToken.current;
    setMorePending(true);
    const pages = expanded ? Array.from({ length: page }, (_, index) => index + 1) : [1];
    void Promise.all(pages.map(number => request(() => client.api.getHistory({ search: '', status: 'all', page: number, pageSize: expanded ? pageSize : 1 })))).then(results => {
      if (token !== historyToken.current) return;
      setMorePending(false);
      const failed = results.find(result => !result.ok);
      if (failed && !failed.ok) { notify(failed.error); return; }
      const values = results.flatMap(result => result.ok ? [result.value] : []);
      setTotal(values[0]?.total ?? 0);
      if (!expanded) return; // Only a cheap count request while collapsed.
      const items = [...new Map(values.flatMap(value => value.items).map(item => [item.id, item])).values()];
      setHistory(items);
      client.updateHistory(items);
      const lastPage = Math.max(1, Math.ceil((values[0]?.total ?? 0) / pageSize));
      if (page > lastPage) setPage(lastPage);
    });
    return () => { historyToken.current++; };
  }, [client, statusRefresh, archiveInvalidation, pageSize, expanded, page]);
  function more() { setPage(value => value + 1); }
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
  async function copyPath(cwd: string) {
    const result = await action.run(() => client.api.copyText({ text: cwd }));
    if (result?.ok) notify({ message: 'Path copied', tone: 'info', durationMs: 2000 });
  }
  function row(session: SessionDto) {
    const editing = renaming === session.id;
    return <div key={session.id} className={`session-row${selectedId === session.id ? ' selected' : ''}`}
      onContextMenu={event => { event.preventDefault(); setMenu({ session, x: event.clientX, y: event.clientY }); }}>
      <StatusDot state={sessionDot(session, busy)} />
      {editing ? <div className="session-copy"><input autoFocus aria-label="Session title" value={name} maxLength={160} disabled={action.pending} onChange={event => setName(event.target.value)} onKeyDown={event => { if (event.key === 'Enter') void saveName(session); if (event.key === 'Escape') setRenaming(null); }} /><small title={session.cwd}>{shortenHome(session.cwd)}</small></div>
        : <button className="session-copy" aria-label={`Select session ${session.title}`} aria-pressed={selectedId === session.id} onClick={() => client.select(session)}><span className="session-title" title={session.title} onDoubleClick={() => rename(session)}>{session.title}</span><small title={session.cwd}>{shortenHome(session.cwd)}</small></button>}
    </div>;
  }
  const live = sessions.filter(session => !session.settledAt && session.status !== 'settled');
  return <div className="sidebar-scroll"><div role="region" aria-label="Sessions">{live.length ? live.map(row) : <p className="empty-small">No sessions — press +</p>}</div>
    <button className="archive-toggle" aria-expanded={expanded} onClick={() => setExpanded(value => !value)}><span>Archived · {total}</span><span className={expanded ? 'expanded' : ''}><Icon name="chevron" /></span></button>
    {expanded && <div role="region" aria-label="Archived sessions">{history.map(row)}{history.length < total && <button className="text-button history-more" disabled={morePending} onClick={() => void more()}>More</button>}</div>}
    {menu && <ContextMenu x={menu.x} y={menu.y} onClose={() => setMenu(null)}>
      <button role="menuitem" disabled={action.pending} onClick={() => rename(menu.session)}>Rename</button>
      <button role="menuitem" disabled={action.pending} onClick={() => { setEditingEnv(menu.session); setMenu(null); }}>Environment variables…</button>
      <button role="menuitem" disabled={action.pending} onClick={() => { const sessionId = menu.session.id; setMenu(null); void action.run(() => client.api.openSessionFolder({ sessionId })); }}>Open in File Explorer</button>
      <button role="menuitem" disabled={action.pending} onClick={() => { const cwd = menu.session.cwd; setMenu(null); void copyPath(cwd); }}>Copy path</button>
      {(menu.session.status === 'error' || menu.session.error || menu.session.tabs.some(tab => tab.error)) && <button role="menuitem" disabled={action.pending} onClick={() => { const session = menu.session; setMenu(null); void action.run(() => client.api.clearSessionError({ sessionId: session.id })).then(result => { if (result?.ok) client.accept(result.value); }); }}>Clear error</button>}
      {!menu.session.settledAt ? <button role="menuitem" disabled={action.pending} onClick={() => void archive(menu.session)}>Archive</button>
        : menu.session.adapterId === 'embedded-pty' && menu.session.tabs.some(tab => tab.terminalKind === 'embedded') && <button role="menuitem" disabled={action.pending} onClick={() => void restore(menu.session)}>Restore</button>}
    </ContextMenu>}
    {editingEnv && <ShellfoxEnvModal key={editingEnv.id} client={client} session={editingEnv} onClose={() => setEditingEnv(null)} />}
    {archiving && <div className="inline-confirm archive-confirm" role="dialog" aria-label="Archive session" onKeyDown={event => { if (event.key === 'Escape') setArchiving(null); }}><p>Agents are still running. Archive anyway?</p><div><button autoFocus disabled={action.pending} onClick={() => void archive(archiving, true)}>Archive</button><button onClick={() => setArchiving(null)}>Cancel</button></div></div>}
  </div>;
}
