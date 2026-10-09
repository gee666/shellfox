import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { KeyboardEvent, ReactNode, RefObject } from 'react';
import { flushSync } from 'react-dom';
import type { SessionDto } from '../shared/contracts';

// Matches .session-row in styles.css. Small lists keep their full DOM.
const ROW_HEIGHT = 36;
const WINDOW_THRESHOLD = 100;
const OVERSCAN = 5;

export function WindowedSessionList({ sessions, scrollRef, selectedId, retainedIds, renderRow }: {
  sessions: SessionDto[];
  scrollRef: RefObject<HTMLDivElement | null>;
  selectedId: string | null;
  retainedIds: (string | null)[];
  renderRow: (session: SessionDto) => ReactNode;
}) {
  const root = useRef<HTMLDivElement>(null);
  const [viewport, setViewport] = useState({ top: 0, height: 360 });
  const [focusedId, setFocusedId] = useState<string | null>(null);
  const focusedControl = useRef<HTMLElement | null>(null);
  const [draggedId, setDraggedId] = useState<string | null>(null);
  const revealedId = useRef<string | null>(null);
  const indices = useMemo(() => new Map(sessions.map((session, index) => [session.id, index])), [sessions]);
  const windowed = sessions.length > WINDOW_THRESHOLD;

  function offset() {
    const scroll = scrollRef.current!;
    return root.current!.getBoundingClientRect().top - scroll.getBoundingClientRect().top + scroll.scrollTop - scroll.clientTop;
  }
  function measure() {
    const scroll = scrollRef.current;
    if (!scroll || !root.current) return;
    const top = scroll.scrollTop - offset();
    const height = scroll.clientHeight;
    setViewport(previous => previous.top === top && previous.height === height ? previous : { top, height });
  }
  function reveal(index: number) {
    const scroll = scrollRef.current!;
    const top = offset() + index * ROW_HEIGHT;
    const height = scroll.clientHeight;
    if (!height) return;
    if (top < scroll.scrollTop) scroll.scrollTop = top;
    else if (top + ROW_HEIGHT > scroll.scrollTop + height) scroll.scrollTop = top + ROW_HEIGHT - height;
    measure();
  }

  // Re-measure after a live-list size change moves the archive, or deletion
  // causes the browser to clamp scrollTop. No scrolling on status-only updates.
  useLayoutEffect(() => {
    measure();
    const prior = focusedControl.current;
    // Saving/cancelling rename replaces the input without a blur event.
    // Restore its replacement, but never take focus from a menu or dialog.
    if (prior && !prior.isConnected && document.activeElement === document.body && focusedId && indices.has(focusedId)) {
      control(focusedId)?.focus({ preventScroll: true });
    }
  });
  // Parent DOM refs aren't attached yet during a child's first layout effect.
  useEffect(() => {
    const scroll = scrollRef.current;
    if (!scroll) return;
    measure();
    let frame: number | undefined;
    const schedule = () => {
      if (frame !== undefined) return;
      frame = requestAnimationFrame(() => { frame = undefined; measure(); });
    };
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(schedule);
    observer?.observe(scroll);
    if (root.current) observer?.observe(root.current);
    scroll.addEventListener('scroll', schedule, { passive: true });
    window.addEventListener('resize', schedule);
    return () => {
      observer?.disconnect();
      scroll.removeEventListener('scroll', schedule);
      window.removeEventListener('resize', schedule);
      if (frame !== undefined) cancelAnimationFrame(frame);
    };
  }, [scrollRef]);
  useEffect(() => {
    if (!selectedId) { revealedId.current = null; return; }
    const index = indices.get(selectedId);
    if (index === undefined) { revealedId.current = null; return; }
    if (revealedId.current === selectedId || !scrollRef.current?.clientHeight) return;
    revealedId.current = selectedId;
    // A small archive can still be offscreen below a large live list.
    reveal(index);
  }, [selectedId, indices, windowed, viewport.height]);

  function rowId(target: EventTarget | null) {
    return target instanceof Element ? target.closest<HTMLElement>('[data-session-id]')?.dataset.sessionId ?? null : null;
  }
  function wrapper(id: string) {
    return Array.from(root.current?.children ?? []).find(node => (node as HTMLElement).dataset.sessionId === id);
  }
  function control(id: string) {
    return wrapper(id)?.querySelector<HTMLElement>('input:not(:disabled), button:not(:disabled)');
  }
  function keyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.defaultPrevented) return;
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    const id = rowId(event.target);
    const current = id ? indices.get(id) : undefined;
    if (current === undefined) return;
    // Keep input editing keys intact. Tab still follows the complete list,
    // including rows that aren't mounted yet.
    const editing = event.target instanceof HTMLInputElement;
    let next: number;
    if (event.key === 'Tab') next = current + (event.shiftKey ? -1 : 1);
    else if (!editing && event.key === 'ArrowDown') next = current + 1;
    else if (!editing && event.key === 'ArrowUp') next = current - 1;
    else if (!editing && event.key === 'Home') next = 0;
    else if (!editing && event.key === 'End') next = sessions.length - 1;
    else return;
    const direction = event.key === 'End' || event.key === 'ArrowUp' || event.key === 'Tab' && event.shiftKey ? -1 : 1;
    // Rename inputs are retained even offscreen and can be disabled while an
    // action runs. Native Tab would skip them; logical traversal must too.
    while (next >= 0 && next < sessions.length) {
      const node = wrapper(sessions[next]!.id);
      if (!node || control(sessions[next]!.id)) break;
      next += direction;
    }
    if (next < 0 || next >= sessions.length) return; // Native Tab leaves the list.
    event.preventDefault();
    const nextId = sessions[next]!.id;
    // Mount before focusing, rather than sending focus to a spacer or body.
    flushSync(() => { setFocusedId(nextId); if (windowed) reveal(next); });
    control(nextId)?.focus();
  }

  const start = Math.max(0, Math.min(sessions.length, Math.floor(viewport.top / ROW_HEIGHT) - OVERSCAN));
  const end = Math.max(start, Math.min(sessions.length, Math.ceil((viewport.top + viewport.height) / ROW_HEIGHT) + OVERSCAN));
  const visible = new Set<number>();
  for (let index = windowed ? start : 0; index < (windowed ? end : sessions.length); index++) visible.add(index);
  // Native Tab can enter from the toolbar, archive toggle or More button.
  // Keep both boundaries mounted so entry never skips an offscreen row.
  if (windowed) { visible.add(0); visible.add(sessions.length - 1); }
  for (const id of [...retainedIds, focusedId, draggedId]) {
    const index = id ? indices.get(id) : undefined;
    if (index !== undefined) visible.add(index);
  }
  const children: ReactNode[] = [];
  let cursor = 0;
  function gap(end: number) {
    if (end > cursor) children.push(<div key={`gap-${cursor}`} aria-hidden="true" style={{ height: (end - cursor) * ROW_HEIGHT }} />);
  }
  for (const index of [...visible].sort((a, b) => a - b)) {
    gap(index);
    const session = sessions[index]!;
    children.push(<div key={session.id} role="listitem" aria-posinset={index + 1} aria-setsize={sessions.length} data-session-id={session.id}>{renderRow(session)}</div>);
    cursor = index + 1;
  }
  gap(sessions.length);
  return <div ref={root} role="list" style={{ overflowAnchor: 'none' }} onKeyDown={keyDown}
    onFocusCapture={event => {
      const id = rowId(event.target);
      focusedControl.current = event.target instanceof HTMLElement ? event.target : null;
      setFocusedId(id);
      const index = id ? indices.get(id) : undefined;
      if (index !== undefined) reveal(index);
    }}
    onBlurCapture={event => {
      const next = event.relatedTarget;
      // Menus/dialogs restore their prior focus in a passive cleanup. Keep
      // that row mounted even in the commit that closes the overlay.
      if (next instanceof Element && next.closest('[role="menu"], [role="dialog"]')) return;
      if (!event.currentTarget.contains(next as Node | null)) { focusedControl.current = null; setFocusedId(null); }
    }}
    onDragStartCapture={event => setDraggedId(rowId(event.target))} onDragEndCapture={() => setDraggedId(null)}>{children}</div>;
}
