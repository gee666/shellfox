import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from 'react';

export const FILE_WIDTH_KEY = 'shellfox.fileSidebarWidth';
export const FILE_BROWSER_HEIGHT_KEY = 'shellfox.fileBrowserHeight';
export const FILE_WIDTH_DEFAULT = 420;
export const FILE_BROWSER_HEIGHT_DEFAULT = 220;

export function clampLayoutSize(value: number, min: number, max: number, fallback: number) {
  return Math.max(min, Math.min(max, Number.isFinite(value) ? value : fallback));
}

function useLayoutPreference(key: string, fallback: number) {
  const [preferred, setPreferred] = useState(() => {
    try {
      const stored = localStorage.getItem(key);
      const value = stored === null || !stored.trim() ? fallback : Number(stored);
      return Number.isFinite(value) && value >= 0 ? value : fallback;
    } catch { return fallback; }
  });
  useEffect(() => {
    try { localStorage.setItem(key, String(preferred)); } catch { /* Storage may be disabled. */ }
  }, [key, preferred]);
  return [preferred, setPreferred] as const;
}

export function useFileLayout(hidden?: boolean) {
  const root = useRef<HTMLElement>(null);
  const [maximized, setMaximized] = useState(false);
  const [preferredWidth, setWidth] = useLayoutPreference(FILE_WIDTH_KEY, FILE_WIDTH_DEFAULT);
  const [preferredHeight, setBrowserHeight] = useLayoutPreference(FILE_BROWSER_HEIGHT_KEY, FILE_BROWSER_HEIGHT_DEFAULT);
  const [bounds, setBounds] = useState(() => ({ workspace: Math.max(0, window.innerWidth - 224), height: window.innerHeight, chrome: 64, viewport: window.innerWidth }));
  useLayoutEffect(() => {
    const element = root.current;
    if (!element) return;
    const measure = () => {
      const workspace = element.parentElement?.getBoundingClientRect().width || Math.max(0, window.innerWidth - 224);
      const height = element.getBoundingClientRect().height || window.innerHeight;
      const chrome = (element.querySelector('.file-sidebar-header')?.getBoundingClientRect().height || 32)
        + (element.querySelector('.file-transfers')?.getBoundingClientRect().height || 32);
      setBounds(previous => previous.workspace === workspace && previous.height === height && previous.chrome === chrome && previous.viewport === window.innerWidth
        ? previous : { workspace, height, chrome, viewport: window.innerWidth });
    };
    measure();
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure);
    observer?.observe(element);
    if (element.parentElement) observer?.observe(element.parentElement);
    const transfers = element.querySelector('.file-transfers');
    if (transfers) observer?.observe(transfers);
    window.addEventListener('resize', measure);
    return () => { observer?.disconnect(); window.removeEventListener('resize', measure); };
  }, [hidden]);
  const narrow = bounds.viewport <= 700 || bounds.workspace < 500;
  const widthMax = Math.max(0, bounds.workspace - 200);
  const widthMin = Math.min(300, widthMax);
  const width = clampLayoutSize(preferredWidth, widthMin, widthMax, FILE_WIDTH_DEFAULT);
  const heightMax = Math.max(0, bounds.height - bounds.chrome - 164);
  const heightMin = Math.min(68, heightMax);
  const browserHeight = clampLayoutSize(preferredHeight, heightMin, heightMax, FILE_BROWSER_HEIGHT_DEFAULT);
  const style: CSSProperties & { '--file-sidebar-width': string; '--file-browser-height': string } = {
    '--file-sidebar-width': `${width}px`, '--file-browser-height': `${browserHeight}px`,
  };
  return { root, maximized, toggleMaximized: () => setMaximized(value => !value), narrow, style,
    width, widthMin, widthMax, setWidth, browserHeight, heightMin, heightMax, setBrowserHeight };
}
