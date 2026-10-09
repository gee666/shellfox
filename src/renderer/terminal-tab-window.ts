import { useCallback, useEffect, useLayoutEffect, useMemo, useState } from 'react';
import type { RefObject } from 'react';
import type { TabDto } from '../shared/contracts';

export const TAB_WINDOW_THRESHOLD = 100;
// TerminalTabItem applies this fixed outer width to large strips. Small strips
// retain their content-sized tabs, so their existing appearance does not change.
export const TAB_WINDOW_WIDTH = 180;
const OVERSCAN = 3;
const FALLBACK_WIDTH = 800;

export function tabWindowIndices(count: number, left: number, width: number, retained: number[]): number[] {
  // A deletion can shrink the strip before the browser clamps scrollLeft.
  const offset = Math.max(0, Math.min(left, Math.max(0, count * TAB_WINDOW_WIDTH - width)));
  const start = Math.max(0, Math.min(count, Math.floor(offset / TAB_WINDOW_WIDTH) - OVERSCAN));
  const end = Math.max(start, Math.min(count, Math.ceil((offset + width) / TAB_WINDOW_WIDTH) + OVERSCAN));
  const indices = new Set<number>();
  for (let index = start; index < end; index++) indices.add(index);
  for (const index of retained) if (index >= 0 && index < count) indices.add(index);
  return [...indices].sort((a, b) => a - b);
}

export function useTerminalTabWindow(root: RefObject<HTMLDivElement | null>, tabs: TabDto[], selectedId: string | undefined, retainedIds: (string | null)[]) {
  const windowed = tabs.length > TAB_WINDOW_THRESHOLD;
  const positions = useMemo(() => new Map(tabs.map((tab, index) => [tab.id, index])), [tabs]);
  const selectedIndex = selectedId ? positions.get(selectedId) : undefined;
  const [viewport, setViewport] = useState({ left: 0, width: FALLBACK_WIDTH });
  const measure = useCallback(() => {
    const node = root.current;
    if (!node) return;
    const left = node.scrollLeft, width = node.clientWidth || FALLBACK_WIDTH;
    setViewport(previous => previous.left === left && previous.width === width ? previous : { left, width });
  }, [root]);
  const reveal = useCallback((index: number) => {
    const node = root.current;
    if (!node) return;
    let left: number, width: number;
    if (windowed) { left = index * TAB_WINDOW_WIDTH; width = TAB_WINDOW_WIDTH; }
    else {
      const tab = node.querySelector<HTMLElement>(`[data-tab-index="${index}"]`);
      if (!tab) return;
      const rect = tab.getBoundingClientRect();
      left = rect.left - node.getBoundingClientRect().left + node.scrollLeft; width = rect.width;
    }
    const visibleWidth = node.clientWidth || FALLBACK_WIDTH;
    if (left < node.scrollLeft) node.scrollLeft = left;
    else if (left + width > node.scrollLeft + visibleWidth) node.scrollLeft = Math.max(0, left + width - visibleWidth);
    measure();
  }, [root, windowed, measure]);

  useLayoutEffect(measure, [measure, tabs.length, windowed]);
  // Selection and reordering reveal the tab. Activity-only snapshots and manual
  // scrolling do not pull the strip back to the selected terminal.
  useLayoutEffect(() => { if (selectedIndex !== undefined) reveal(selectedIndex); }, [selectedId, selectedIndex, viewport.width, reveal]);
  useEffect(() => {
    const node = root.current;
    if (!node) return;
    let frame: number | null = null;
    const schedule = () => {
      if (frame !== null) return;
      frame = requestAnimationFrame(() => { frame = null; measure(); });
    };
    const observer = new ResizeObserver(schedule); observer.observe(node);
    node.addEventListener('scroll', schedule, { passive: true });
    return () => { observer.disconnect(); node.removeEventListener('scroll', schedule); if (frame !== null) cancelAnimationFrame(frame); };
  }, [root, measure]);

  const retained = [selectedId, ...retainedIds].flatMap(id => {
    const index = id ? positions.get(id) : undefined;
    return index === undefined ? [] : [index];
  });
  const visible = windowed ? tabWindowIndices(tabs.length, viewport.left, viewport.width, retained) : tabs.map((_, index) => index);
  return { windowed, positions, visible, reveal, measure };
}
