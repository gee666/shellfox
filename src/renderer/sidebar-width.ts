import { useEffect, useState } from 'react';

import { readAppStorage } from './storage-upgrade';

export const SIDEBAR_WIDTH_KEY = 'shellfox.sidebarWidth';
export function clampSidebarWidth(value: number, windowWidth = window.innerWidth) {
  return Math.max(160, Math.min(Number.isFinite(value) ? value : 220, Math.max(160, windowWidth / 2)));
}
export function useSidebarWidth() {
  const [width, setWidth] = useState(() => {
    try {
      const stored = readAppStorage(localStorage, SIDEBAR_WIDTH_KEY);
      return clampSidebarWidth(stored === null ? 220 : Number(stored));
    } catch { return clampSidebarWidth(220); }
  });
  useEffect(() => {
    try { localStorage.setItem(SIDEBAR_WIDTH_KEY, String(width)); } catch { /* Storage may be disabled. */ }
  }, [width]);
  useEffect(() => {
    const resize = () => setWidth(value => clampSidebarWidth(value));
    window.addEventListener('resize', resize);
    return () => window.removeEventListener('resize', resize);
  }, []);
  return [width, (value: number) => setWidth(clampSidebarWidth(value))] as const;
}
