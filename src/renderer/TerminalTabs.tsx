import { memo, useEffect, useRef, useState } from 'react';
import type { DragEvent, KeyboardEvent, ReactNode } from 'react';
import { flushSync } from 'react-dom';
import type { SessionDto, TabDto } from '../shared/contracts';
import { failure } from '../shared/contracts';
import { titleSchema } from '../shared/schemas';
import type { ManagerClient } from './store';
import { ContextMenu, Icon, StatusDot, tabDot, useAction } from './components';
import type { DotState } from './components';
import { TAB_WINDOW_WIDTH, useTerminalTabWindow } from './terminal-tab-window';

// Primitive display props survive fresh metadata DTOs and busy-map updates.
// Events are delegated to the strip, so memoized rows never hold stale actions.
const TerminalTabItem = memo(function TerminalTabItem({ id, title, index, count, active, closeable, locked, editable, editing, dropTarget, dotKind, dotTitle, windowed, editor }: {
  id: string; title: string; index: number; count: number; active: boolean; closeable: boolean;
  locked: boolean; editable: boolean; editing: boolean; dropTarget: boolean;
  dotKind: DotState['kind']; dotTitle: string; windowed: boolean; editor: ReactNode;
}) {
  return <div data-tab-id={id} data-tab-index={index} className={`terminal-tab-item${active ? ' active' : ''}${dropTarget ? ' tab-drop-target' : ''}`}
    style={windowed ? { flexBasis: TAB_WINDOW_WIDTH, width: TAB_WINDOW_WIDTH, minWidth: TAB_WINDOW_WIDTH, maxWidth: TAB_WINDOW_WIDTH } : undefined}>
    <button role="tab" id={`terminal-tab-${id}`} aria-selected={active} aria-controls={`terminal-panel-${id}`} aria-posinset={index + 1} aria-setsize={count} tabIndex={active ? 0 : -1}
      style={editor ? { display: 'none' } : undefined} draggable={editable && !locked && !editing}
      title={`${title}. Double-click or F2 to rename. Drag or Alt+Arrow to reorder.`}><StatusDot state={{ kind: dotKind, title: dotTitle }} /><span>{title}</span></button>
    {editor}
    {closeable && <button className="icon-button tab-close" aria-label={`Close terminal ${title}`} disabled={locked || editing} tabIndex={windowed && !active ? -1 : undefined}><Icon name="close" /></button>}
  </div>;
});

export function TerminalTabs({ session, tabs, selectedId, client, busy, disabled, editable, canClose, onSelect, onClose }: {
  session: SessionDto; tabs: TabDto[]; selectedId?: string; client: ManagerClient; busy: Record<string, boolean>;
  disabled: boolean; editable: boolean; canClose: (tab: TabDto) => boolean;
  onSelect: (tab: TabDto) => void; onClose: (tab: TabDto) => void;
}) {
  const action = useAction();
  const [editing, setEditing] = useState<{ id: string; title: string } | null>(null);
  const [menu, setMenu] = useState<{ id: string; x: number; y: number } | null>(null);
  const dragged = useRef<string | null>(null);
  const [draggedId, setDraggedId] = useState<string | null>(null);
  const [focusedId, setFocusedId] = useState<string | null>(null);
  const [dropTarget, setDropTarget] = useState<string | null>(null);
  const committing = useRef(false);
  const cancelled = useRef(false);
  const input = useRef<HTMLInputElement>(null);
  const restoreFocus = useRef<string | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const window = useTerminalTabWindow(root, tabs, selectedId, [editing?.id ?? null, menu?.id ?? null, focusedId, draggedId, restoreFocus.current]);
  const dragFrame = useRef<number | null>(null);
  const dragStep = useRef(0);
  const locked = disabled || action.pending;
  const findTab = (id: string | null) => { const index = id ? window.positions.get(id) : undefined; return index === undefined ? undefined : tabs[index]; };
  const menuTab = findTab(menu?.id ?? null);

  useEffect(() => {
    if (editing) { input.current?.focus(); input.current?.select(); }
    else if (restoreFocus.current) { focusTab(restoreFocus.current); restoreFocus.current = null; }
  }, [editing?.id]);
  useEffect(() => {
    if (editing && (!editable || !window.positions.has(editing.id))) { cancelled.current = true; setEditing(null); }
    if (menu && !menuTab) setMenu(null);
  }, [editable, window.positions, editing?.id, menu?.id]);
  useEffect(() => () => stopDragScroll(), []);
  useEffect(() => { if (!editable || locked || dragged.current && !window.positions.has(dragged.current)) clearDrag(); }, [editable, locked, window.positions]);

  function focusTab(id: string) {
    const index = window.positions.get(id);
    if (index === undefined) return;
    window.reveal(index);
    root.current?.querySelector<HTMLButtonElement>(`[data-tab-index="${index}"] [role="tab"]`)?.focus({ preventScroll: true });
  }
  function rename(tab: TabDto) {
    if (!editable || locked) return;
    cancelled.current = false; setMenu(null); setEditing({ id: tab.id, title: tab.title });
  }
  function cancel() { cancelled.current = true; restoreFocus.current = editing?.id ?? null; setEditing(null); }
  async function save() {
    if (!editing || cancelled.current || committing.current || locked) return;
    const draft = editing;
    if (!titleSchema.safeParse(draft.title).success) {
      await action.run(async () => failure('VALIDATION', 'Use a nonblank title of at most 200 characters, without control characters.'));
      return;
    }
    const title = draft.title.trim();
    // Saving the displayed process title also makes it a user override.
    committing.current = true;
    try {
      const result = await action.run(() => client.api.renameTab({ sessionId: session.id, tabId: draft.id, title }));
      if (result?.ok) {
        restoreFocus.current = draft.id; setEditing(null);
        if (client.store.getState().selectedId === session.id) client.accept(result.value);
      }
      client.refresh();
    } finally { committing.current = false; }
  }
  async function move(source: string, target: string) {
    if (!editable || locked || editing || source === target) return;
    const ids = tabs.map(tab => tab.id), from = window.positions.get(source), to = window.positions.get(target);
    if (from === undefined || to === undefined) return;
    ids.splice(from, 1); ids.splice(to, 0, source);
    // Hidden closed/legacy tabs keep their slots in the complete saved permutation.
    const visible = new Set(ids);
    let index = 0;
    const tabIds = session.tabs.map(tab => visible.has(tab.id) ? ids[index++]! : tab.id);
    const result = await action.run(() => client.api.reorderTabs({ sessionId: session.id, tabIds }));
    if (result?.ok && client.store.getState().selectedId === session.id) client.accept(result.value);
    client.refresh();
  }
  function key(event: KeyboardEvent<HTMLDivElement>, tab: TabDto, index: number) {
    if (event.key === 'F2') { event.preventDefault(); rename(tab); return; }
    if (event.key === 'ContextMenu' || event.shiftKey && event.key === 'F10') {
      event.preventDefault(); const rect = (event.target as HTMLElement).getBoundingClientRect();
      if (editable && !locked) setMenu({ id: tab.id, x: rect.left, y: rect.bottom });
      return;
    }
    if (event.altKey && ['ArrowLeft', 'ArrowRight'].includes(event.key)) {
      event.preventDefault(); const target = tabs[index + (event.key === 'ArrowLeft' ? -1 : 1)];
      if (target) void move(tab.id, target.id);
      return;
    }
    const target = event.key === 'ArrowRight' ? (index + 1) % tabs.length : event.key === 'ArrowLeft' ? (index + tabs.length - 1) % tabs.length : event.key === 'Home' ? 0 : event.key === 'End' ? tabs.length - 1 : undefined;
    if (target === undefined) return;
    event.preventDefault();
    const next = tabs[target]!;
    // An offscreen destination must be mounted before focus is moved.
    flushSync(() => { setFocusedId(next.id); window.reveal(target); onSelect(next); });
    focusTab(next.id);
  }
  function tabAt(target: EventTarget | null) {
    const row = target instanceof Element ? target.closest<HTMLElement>('[data-tab-id]') : null;
    return row && root.current?.contains(row) ? findTab(row.dataset.tabId ?? null) : undefined;
  }
  function tabButton(target: EventTarget | null) {
    const button = target instanceof Element ? target.closest('button') : null;
    return button?.getAttribute('role') === 'tab';
  }
  function stopDragScroll() {
    dragStep.current = 0;
    if (dragFrame.current !== null) cancelAnimationFrame(dragFrame.current);
    dragFrame.current = null;
  }
  function clearDrag() { dragged.current = null; setDraggedId(null); setDropTarget(null); stopDragScroll(); }
  function dragScroll() {
    dragFrame.current = null;
    const node = root.current;
    if (!node || !dragged.current || !dragStep.current) return;
    const before = node.scrollLeft; node.scrollLeft += dragStep.current; window.measure();
    if (node.scrollLeft !== before) dragFrame.current = requestAnimationFrame(dragScroll);
  }
  function edgeScroll(x: number) {
    const rect = root.current!.getBoundingClientRect();
    dragStep.current = x < rect.left + 32 ? -18 : x > rect.right - 32 ? 18 : 0;
    if (!dragStep.current) stopDragScroll();
    else if (dragFrame.current === null) dragFrame.current = requestAnimationFrame(dragScroll);
  }
  function dropTab(event: DragEvent<HTMLDivElement>) {
    const tab = tabAt(event.target);
    if (tab || !window.windowed) return tab;
    // A fast scroll may leave the pointer over a spacer for one frame.
    const index = Math.floor((event.clientX - root.current!.getBoundingClientRect().left + root.current!.scrollLeft) / TAB_WINDOW_WIDTH);
    return tabs[Math.max(0, Math.min(tabs.length - 1, index))];
  }

  const children: ReactNode[] = [];
  let cursor = 0;
  function gap(end: number) {
    if (window.windowed && end > cursor) children.push(<div key={`gap-${cursor}`} className="terminal-tab-spacer" aria-hidden="true" style={{ width: (end - cursor) * TAB_WINDOW_WIDTH }} />);
  }
  for (const index of window.visible) {
    gap(index);
    const tab = tabs[index]!, dot = tabDot(tab, busy[tab.id]);
    const editor = editing?.id === tab.id ? <input ref={input} className="tab-rename" aria-label="Terminal tab name" value={editing.title} maxLength={200}
      onFocus={event => event.currentTarget.select()} disabled={action.pending} onChange={event => setEditing({ ...editing, title: event.target.value })}
      onBlur={() => void save()} onKeyDown={event => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); cancel(); } else if (event.key === 'Enter') { event.preventDefault(); void save(); } }} /> : null;
    children.push(<TerminalTabItem key={tab.id} id={tab.id} title={tab.title} index={index} count={tabs.length} active={selectedId === tab.id} closeable={canClose(tab)} locked={locked} editable={editable}
      editing={!!editing} dropTarget={dropTarget === tab.id} dotKind={dot.kind} dotTitle={dot.title} windowed={window.windowed} editor={editor} />);
    cursor = index + 1;
  }
  gap(tabs.length);
  return <><div ref={root} role="tablist" aria-label="Terminals" className={`terminal-tabs${window.windowed ? ' terminal-tabs-windowed' : ''}`}
    onClick={event => { const tab = tabAt(event.target); if (!tab) return; if (tabButton(event.target)) onSelect(tab); else if ((event.target as Element).closest('.tab-close') && !locked && !editing && canClose(tab)) onClose(tab); }}
    onDoubleClick={event => { const tab = tabAt(event.target); if (tab && tabButton(event.target)) rename(tab); }}
    onKeyDown={event => { const tab = tabAt(event.target); if (tab && tabButton(event.target)) key(event, tab, window.positions.get(tab.id)!); }}
    onContextMenu={event => { event.preventDefault(); const tab = tabAt(event.target); if (tab && editable && !locked && !editing) setMenu({ id: tab.id, x: event.clientX, y: event.clientY }); }}
    onFocusCapture={event => { const tab = tabAt(event.target); if (tab) { setFocusedId(tab.id); window.reveal(window.positions.get(tab.id)!); } }}
    onBlurCapture={event => { const next = event.relatedTarget; if (next instanceof Element && next.closest('[role="menu"], [role="dialog"]')) return; if (!event.currentTarget.contains(next as Node | null)) setFocusedId(null); }}
    onAuxClick={event => { const tab = tabAt(event.target); if (tab && event.button === 1 && canClose(tab) && !locked && !editing) { event.preventDefault(); onClose(tab); } }}
    onMouseDown={event => { if (event.button === 1 && tabAt(event.target)) event.preventDefault(); }}
    onDragStart={event => { const tab = tabAt(event.target); if (!tab || !tabButton(event.target) || !editable || locked || editing) { event.preventDefault(); return; } dragged.current = tab.id; setDraggedId(tab.id); event.dataTransfer.effectAllowed = 'move'; event.dataTransfer.setData('application/x-shellfox-tab', tab.id); }}
    onDragOver={event => { if (!editable || locked || !dragged.current || editing) return; event.preventDefault(); event.dataTransfer.dropEffect = 'move'; const target = dropTab(event); setDropTarget(target?.id === dragged.current ? null : target?.id ?? null); edgeScroll(event.clientX); }}
    onDragLeave={event => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) { setDropTarget(null); stopDragScroll(); } }}
    onDrop={event => { event.preventDefault(); const source = dragged.current, target = dropTab(event); clearDrag(); if (source && target) void move(source, target.id); }} onDragEnd={clearDrag}>{children}</div>
    {menu && menuTab && <ContextMenu x={menu.x} y={menu.y} onClose={() => setMenu(null)}>
      <button role="menuitem" disabled={locked} onClick={() => rename(menuTab)}>Rename terminal</button>
      <button role="menuitem" disabled={locked || tabs[0]?.id === menu.id} onClick={() => { const index = window.positions.get(menu.id)!; setMenu(null); if (tabs[index - 1]) void move(menu.id, tabs[index - 1]!.id); }}>Move left</button>
      <button role="menuitem" disabled={locked || tabs[tabs.length - 1]?.id === menu.id} onClick={() => { const index = window.positions.get(menu.id)!; setMenu(null); if (tabs[index + 1]) void move(menu.id, tabs[index + 1]!.id); }}>Move right</button>
    </ContextMenu>}</>;
}
