import { useEffect, useRef, useState } from 'react';
import type { KeyboardEvent } from 'react';
import type { SessionDto, TabDto } from '../shared/contracts';
import { failure } from '../shared/contracts';
import { titleSchema } from '../shared/schemas';
import type { ManagerClient } from './store';
import { ContextMenu, Icon, StatusDot, tabDot, useAction } from './components';

export function TerminalTabs({ session, tabs, selectedId, client, busy, disabled, editable, canClose, onSelect, onClose }: {
  session: SessionDto; tabs: TabDto[]; selectedId?: string; client: ManagerClient; busy: Record<string, boolean>;
  disabled: boolean; editable: boolean; canClose: (tab: TabDto) => boolean;
  onSelect: (tab: TabDto) => void; onClose: (tab: TabDto) => void;
}) {
  const action = useAction();
  const [editing, setEditing] = useState<{ id: string; title: string } | null>(null);
  const [menu, setMenu] = useState<{ tab: TabDto; x: number; y: number } | null>(null);
  const dragged = useRef<string | null>(null);
  const [dropTarget, setDropTarget] = useState<string | null>(null);
  const committing = useRef(false);
  const cancelled = useRef(false);
  const input = useRef<HTMLInputElement>(null);
  const restoreFocus = useRef<string | null>(null);
  useEffect(() => {
    if (editing) { input.current?.focus(); input.current?.select(); }
    else if (restoreFocus.current) { focusTab(restoreFocus.current); restoreFocus.current = null; }
  }, [editing?.id]);
  const locked = disabled || action.pending;
  useEffect(() => {
    if (editing && (!editable || !tabs.some(tab => tab.id === editing.id))) {
      cancelled.current = true; setEditing(null);
    }
  }, [editable, tabs, editing?.id]);
  function focusTab(id: string) { document.getElementById(`terminal-tab-${id}`)?.focus(); }
  function rename(tab: TabDto) {
    if (!editable || locked) return;
    cancelled.current = false; setMenu(null); setEditing({ id: tab.id, title: tab.title });
  }
  function cancel() {
    cancelled.current = true; restoreFocus.current = editing?.id ?? null; setEditing(null);
  }
  async function save() {
    if (!editing || cancelled.current || committing.current || locked) return;
    const draft = editing;
    if (!titleSchema.safeParse(draft.title).success) {
      await action.run(async () => failure('VALIDATION', 'Use a nonblank title of at most 200 characters, without control characters.'));
      return;
    }
    const title = draft.title.trim();
    if (tabs.find(tab => tab.id === draft.id)?.title === title) { cancel(); return; }
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
    const ids = tabs.map(tab => tab.id), from = ids.indexOf(source), to = ids.indexOf(target);
    if (from < 0 || to < 0) return;
    ids.splice(from, 1); ids.splice(to, 0, source);
    // Hidden closed/legacy tabs keep their slots in the complete saved permutation.
    const visible = new Set(ids);
    let index = 0;
    const tabIds = session.tabs.map(tab => visible.has(tab.id) ? ids[index++]! : tab.id);
    const result = await action.run(() => client.api.reorderTabs({ sessionId: session.id, tabIds }));
    if (result?.ok && client.store.getState().selectedId === session.id) client.accept(result.value);
    client.refresh();
  }
  function key(event: KeyboardEvent<HTMLButtonElement>, tab: TabDto, index: number) {
    if (event.key === 'F2') { event.preventDefault(); rename(tab); return; }
    if (event.key === 'ContextMenu' || event.shiftKey && event.key === 'F10') {
      event.preventDefault(); const rect = event.currentTarget.getBoundingClientRect();
      if (editable) setMenu({ tab, x: rect.left, y: rect.bottom });
      return;
    }
    if (event.altKey && ['ArrowLeft', 'ArrowRight'].includes(event.key)) {
      event.preventDefault(); const target = tabs[index + (event.key === 'ArrowLeft' ? -1 : 1)];
      if (target) void move(tab.id, target.id);
      return;
    }
    const target = event.key === 'ArrowRight' ? (index + 1) % tabs.length : event.key === 'ArrowLeft' ? (index + tabs.length - 1) % tabs.length : event.key === 'Home' ? 0 : event.key === 'End' ? tabs.length - 1 : undefined;
    if (target === undefined) return;
    event.preventDefault(); onSelect(tabs[target]!); focusTab(tabs[target]!.id);
  }
  return <><div role="tablist" aria-label="Terminals" className="terminal-tabs">{tabs.map((tab, index) => {
    const active = selectedId === tab.id;
    return <div key={tab.id} className={`terminal-tab-item${active ? ' active' : ''}${dropTarget === tab.id ? ' tab-drop-target' : ''}`}
      onContextMenu={event => { event.preventDefault(); if (editable && !locked && !editing) setMenu({ tab, x: event.clientX, y: event.clientY }); }}
      onDragOver={event => { if (editable && !locked && dragged.current && dragged.current !== tab.id) { event.preventDefault(); event.dataTransfer.dropEffect = 'move'; setDropTarget(tab.id); } }}
      onDragLeave={() => setDropTarget(null)}
      onDrop={event => { event.preventDefault(); const source = dragged.current; dragged.current = null; setDropTarget(null); if (source) void move(source, tab.id); }}
      onAuxClick={event => { if (event.button === 1 && canClose(tab) && !locked && !editing) { event.preventDefault(); onClose(tab); } }}
      onMouseDown={event => { if (event.button === 1) event.preventDefault(); }}>
      <button role="tab" id={`terminal-tab-${tab.id}`} aria-selected={active} aria-controls={`terminal-panel-${tab.id}`} tabIndex={active ? 0 : -1}
        style={editing?.id === tab.id ? { display: 'none' } : undefined} onClick={() => onSelect(tab)} onDoubleClick={() => rename(tab)} onKeyDown={event => key(event, tab, index)}
        draggable={editable && !locked && !editing} onDragStart={event => {
          if (!editable || locked || editing) { event.preventDefault(); return; }
          dragged.current = tab.id; event.dataTransfer.effectAllowed = 'move'; event.dataTransfer.setData('application/x-shellfox-tab', tab.id);
        }} onDragEnd={() => { dragged.current = null; setDropTarget(null); }}
        title={`${tab.title}. Double-click or F2 to rename. Drag or Alt+Arrow to reorder.`}><StatusDot state={tabDot(tab, busy[tab.id])} /><span>{tab.title}</span></button>
      {editing?.id === tab.id && <input ref={input} className="tab-rename" aria-label="Terminal tab name" value={editing.title} maxLength={200}
        onFocus={event => event.currentTarget.select()} disabled={action.pending} onChange={event => setEditing({ ...editing, title: event.target.value })}
        onBlur={() => void save()} onKeyDown={event => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); cancel(); } else if (event.key === 'Enter') { event.preventDefault(); void save(); } }} />}
      {canClose(tab) && <button className="icon-button tab-close" aria-label={`Close terminal ${tab.title}`} disabled={locked || !!editing} onClick={() => onClose(tab)}><Icon name="close" /></button>}
    </div>;
  })}</div>{menu && <ContextMenu x={menu.x} y={menu.y} onClose={() => setMenu(null)}>
    <button role="menuitem" disabled={locked} onClick={() => rename(menu.tab)}>Rename terminal</button>
    <button role="menuitem" disabled={locked || tabs[0]?.id === menu.tab.id} onClick={() => { const index = tabs.findIndex(tab => tab.id === menu.tab.id); setMenu(null); if (tabs[index - 1]) void move(menu.tab.id, tabs[index - 1]!.id); }}>Move left</button>
    <button role="menuitem" disabled={locked || tabs[tabs.length - 1]?.id === menu.tab.id} onClick={() => { const index = tabs.findIndex(tab => tab.id === menu.tab.id); setMenu(null); if (tabs[index + 1]) void move(menu.tab.id, tabs[index + 1]!.id); }}>Move right</button>
  </ContextMenu>}</>;
}
