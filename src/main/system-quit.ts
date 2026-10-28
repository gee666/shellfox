import { powerMonitor, type BrowserWindow } from 'electron';

/**
 * Calls quit when the OS ends Shellfox: systemd stop or OOM policy, logout or
 * shutdown. A cancellable prompt cannot keep the app alive then, so callers
 * must close shells without asking. SIGINT stays interactive.
 *
 * Register after app ready: Electron installs its own signal handlers during
 * startup, and Node handlers registered later replace them.
 */
export function onSystemQuit(window: BrowserWindow, quit: () => void, platform = process.platform): () => void {
  const signals: NodeJS.Signals[] = platform === 'win32' ? [] : ['SIGTERM', 'SIGHUP'];
  // Electron passes a documented but untyped event; preventDefault delays the
  // logind/macOS shutdown until the app has exited.
  const shutdown = (event?: { preventDefault(): void }) => { event?.preventDefault(); quit(); };
  for (const signal of signals) process.on(signal, quit);
  if (platform === 'win32') window.on('session-end', quit); else powerMonitor.on('shutdown', shutdown);
  return () => {
    for (const signal of signals) process.removeListener(signal, quit);
    if (platform === 'win32') window.removeListener('session-end', quit); else powerMonitor.removeListener('shutdown', shutdown);
  };
}
