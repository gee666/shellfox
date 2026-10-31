import type { BrowserWindow } from 'electron';

/**
 * Electron throws from `webContents.mainFrame` (and `send`, which goes through it) once the
 * render frame is disposed, e.g. while the renderer crashes or reloads. Treat that as "not showing".
 */
export function rendererShowing(window: BrowserWindow, rendererUrl: string): boolean {
  try { return !window.isDestroyed() && window.webContents.mainFrame.url === rendererUrl; }
  catch { return false; }
}

/** Best-effort push: a renderer that is gone or reloading reads fresh state when it loads. */
export function sendToRenderer(window: BrowserWindow, rendererUrl: string, channel: string, value: unknown): void {
  if (!rendererShowing(window, rendererUrl)) return;
  try { window.webContents.send(channel, value); }
  catch { /* frame disposed between the check and the send */ }
}
