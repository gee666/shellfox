import { dialog, type BrowserWindow, type RenderProcessGoneDetails } from 'electron';

export interface RendererRecoveryOptions {
  /** Reloads allowed inside `windowMs` before asking the user to retry. */
  maxReloads?: number; windowMs?: number; delayMs?: number; now?: () => number;
}

const retryUrl = 'https://shellfox.invalid/renderer-retry';
const recoveryPage = `<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'">
<title>Shellfox recovery</title><style>
:root { color-scheme: dark light; font: 14px system-ui, sans-serif; }
body { margin: 0; min-height: 100vh; display: grid; place-items: center; background: #111016; color: #eee; }
main { padding: 24px; max-width: 420px; } h1 { font-size: 18px; } p { line-height: 1.5; color: #bbb; }
a { display: inline-block; padding: 6px 18px; border: 1px solid #888; border-radius: 3px; color: #eee; text-decoration: none; }
a:hover, a:focus-visible { background: #333; outline: 2px solid #aaa; outline-offset: 2px; }
</style></head><body><main><h1>Shellfox's display stopped</h1>
<p>The display kept crashing, so automatic retries have stopped. Your terminal sessions are still running.</p>
<a href="${retryUrl}">Retry</a></main></body></html>`;
const recoveryUrl = `data:text/html;charset=utf-8,${encodeURIComponent(recoveryPage)}`;

/** Terminals live outside the renderer. Recover their view without restarting the host. */
export function recoverRenderer(window: BrowserWindow, rendererUrl: string, options: RendererRecoveryOptions = {}): void {
  const { maxReloads = 3, windowMs = 60_000, delayMs = 500, now = Date.now } = options;
  let reloads: number[] = [];
  let timer: ReturnType<typeof setTimeout> | undefined;
  let showingRecovery = false, nativeRecoveryShown = false;
  const load = (url: string) => {
    if (!window.isDestroyed()) void window.loadURL(url).catch(error => console.error('Renderer recovery navigation failed:', error));
  };
  const retry = () => {
    if (window.isDestroyed()) return;
    showingRecovery = false; nativeRecoveryShown = false;
    reloads = [];
    clearTimeout(timer);
    load(rendererUrl);
  };
  window.webContents.on('will-navigate', (event, url) => {
    if (!showingRecovery || url !== retryUrl) return;
    event.preventDefault();
    retry();
  });
  const onGone = (_event: unknown, details: RenderProcessGoneDetails) => {
    console.error(`Renderer process gone (${details.reason}, exit ${details.exitCode}).`);
    if (details.reason === 'clean-exit' || window.isDestroyed()) return;
    clearTimeout(timer);
    const at = now();
    reloads = reloads.filter(time => at - time < windowMs);
    if (showingRecovery) {
      // A persistent OOM may kill even the static recovery page. Use an OS
      // dialog instead of starting another renderer crash loop.
      if (!nativeRecoveryShown) {
        nativeRecoveryShown = true;
        void dialog.showMessageBox(window, {
          type: 'error', title: 'Shellfox recovery', message: "Shellfox's display stopped",
          detail: 'Your terminal sessions are still running. Retry when enough memory is available.',
          buttons: ['Retry', 'Not now'], defaultId: 0, cancelId: 1,
        }).then(result => { if (result.response === 0) retry(); })
          .catch(error => console.error('Renderer recovery dialog failed:', error));
      }
      return;
    }
    if (reloads.length >= maxReloads) {
      console.error('Renderer keeps crashing; automatic reload stopped.');
      showingRecovery = true;
      load(recoveryUrl);
      return;
    }
    reloads.push(at);
    timer = setTimeout(() => { load(rendererUrl); }, delayMs);
  };
  window.webContents.on('render-process-gone', onGone);
  window.on('closed', () => { clearTimeout(timer); });
}
