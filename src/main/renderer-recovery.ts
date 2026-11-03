import type { BrowserWindow, RenderProcessGoneDetails } from 'electron';

export interface RendererRecoveryOptions {
  /** Reloads allowed inside `windowMs` before giving up, so a crash loop cannot spin forever. */
  maxReloads?: number; windowMs?: number; delayMs?: number; now?: () => number;
}

/**
 * Terminals live in the main process, so a dead renderer only loses the view.
 * Reload it instead of leaving a blank window that can only be fixed by a restart.
 */
export function recoverRenderer(window: BrowserWindow, options: RendererRecoveryOptions = {}): void {
  const { maxReloads = 3, windowMs = 60_000, delayMs = 500, now = Date.now } = options;
  let reloads: number[] = [];
  let timer: ReturnType<typeof setTimeout> | undefined;
  const onGone = (_event: unknown, details: RenderProcessGoneDetails) => {
    console.error(`Renderer process gone (${details.reason}, exit ${details.exitCode}).`);
    if (details.reason === 'clean-exit' || window.isDestroyed()) return;
    const at = now();
    reloads = reloads.filter(time => at - time < windowMs);
    if (reloads.length >= maxReloads) { console.error('Renderer keeps crashing; automatic reload stopped.'); return; }
    reloads.push(at);
    clearTimeout(timer);
    timer = setTimeout(() => { if (!window.isDestroyed()) window.webContents.reload(); }, delayMs);
  };
  window.webContents.on('render-process-gone', onGone);
}
