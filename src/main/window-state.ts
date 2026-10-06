export interface WindowState { x: number; y: number; width: number; height: number; maximized: boolean }
export interface Rect { x: number; y: number; width: number; height: number }
export interface RestoredBounds { options: { x?: number; y?: number; width: number; height: number }; maximized: boolean }

export const DEFAULT_WIDTH = 1220;
export const DEFAULT_HEIGHT = 820;
export const MIN_WIDTH = 850;
export const MIN_HEIGHT = 600;
const MAX_SIZE = 20000;
const MAX_COORD = 100000;
const TITLE_STRIP = 40;
const MIN_VISIBLE_WIDTH = 100;

const int = (v: unknown, min: number, max: number): v is number => typeof v === 'number' && Number.isInteger(v) && v >= min && v <= max;

export function parseWindowState(value: unknown): WindowState | null {
  if (typeof value !== 'object' || value === null) return null;
  const v = value as Record<string, unknown>;
  if (!int(v.x, -MAX_COORD, MAX_COORD) || !int(v.y, -MAX_COORD, MAX_COORD)) return null;
  if (!int(v.width, MIN_WIDTH, MAX_SIZE) || !int(v.height, MIN_HEIGHT, MAX_SIZE)) return null;
  if (typeof v.maximized !== 'boolean') return null;
  return { x: v.x, y: v.y, width: v.width, height: v.height, maximized: v.maximized };
}

export function restoreBounds(saved: WindowState | null, displays: Rect[], primary: Rect): RestoredBounds {
  if (!saved) return { options: { width: DEFAULT_WIDTH, height: DEFAULT_HEIGHT }, maximized: false };
  const strip = { x: saved.x, y: saved.y, width: saved.width, height: TITLE_STRIP };
  const visible = displays.some(d => {
    const w = Math.min(strip.x + strip.width, d.x + d.width) - Math.max(strip.x, d.x);
    const h = Math.min(strip.y + strip.height, d.y + d.height) - Math.max(strip.y, d.y);
    return w >= Math.min(MIN_VISIBLE_WIDTH, saved.width) && h >= 1;
  });
  if (visible) return { options: { x: saved.x, y: saved.y, width: saved.width, height: saved.height }, maximized: saved.maximized };
  return {
    options: { width: Math.max(MIN_WIDTH, Math.min(saved.width, primary.width)), height: Math.max(MIN_HEIGHT, Math.min(saved.height, primary.height)) },
    maximized: saved.maximized,
  };
}

export interface TrackableWindow {
  on(event: string, listener: () => void): unknown;
  removeListener(event: string, listener: () => void): unknown;
  isDestroyed(): boolean;
  isMinimized(): boolean;
  isMaximized(): boolean;
  isFullScreen(): boolean;
  getBounds(): Rect;
  getNormalBounds(): Rect;
}

export interface WindowStateTracker { flush(): void; dispose(): void; stop(): void }

const EVENTS = ['resize', 'move', 'maximize', 'unmaximize'] as const;
const DEBOUNCE_MS = 400;

export function trackWindowState(window: TrackableWindow, save: (state: WindowState) => void): WindowStateTracker {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let disposed = false;
  const capture = (): void => {
    try {
      if (window.isDestroyed() || window.isMinimized()) return;
      const maximized = window.isMaximized() || window.isFullScreen();
      const b = maximized ? window.getNormalBounds() : window.getBounds();
      save({ x: Math.round(b.x), y: Math.round(b.y), width: Math.round(b.width), height: Math.round(b.height), maximized });
    } catch { /* the database may already be closed */ }
  };
  const schedule = (): void => {
    if (disposed) return;
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => { timer = undefined; capture(); }, DEBOUNCE_MS);
  };
  const flush = (): void => {
    if (timer) { clearTimeout(timer); timer = undefined; }
    capture();
  };
  const onClose = (): void => flush();
  for (const e of EVENTS) window.on(e, schedule);
  window.on('close', onClose);
  const dispose = (): void => {
    if (disposed) return;
    flush();
    disposed = true;
    for (const e of EVENTS) window.removeListener(e, schedule);
    window.removeListener('close', onClose);
  };
  return { flush, dispose, stop: dispose };
}
