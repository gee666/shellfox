import { app, BrowserWindow, dialog, net, screen } from 'electron';
import { readFileSync, renameSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import type { HostClient } from '../host/client';
import { EarlyRequestQueue, handleCliRequest } from './cli';
import { installAttachedHostIpc } from './attached-host-ipc';
import { installDesktopPolicy, secureDesktopWindow } from './desktop-security';
import { CloseConfirmationCoordinator } from './close-confirmation';
import { sendToRenderer } from './renderer-channel';
import { parseWindowState, restoreBounds, trackWindowState } from './window-state';
import { UpdateChecker } from './update-check';
import { SelfUpdater, type UpdateSource } from './update/self-updater';
import { createUpdatePlatform } from './update/platform-installer';
import { UpdateShutdown } from './update/update-shutdown';
declare const __TEST_BUILD__: boolean;

export interface AttachedDesktopOptions {
  client: HostClient; requests: EarlyRequestQueue; userData: string; backgroundTest: boolean; confirmationAutoApprove: boolean;
}
/** Attaching is deliberately separate from startup that opens SQLite and PTYs. */
export async function runAttachedDesktop(options: AttachedDesktopOptions): Promise<void> {
  const { client, requests, userData, backgroundTest } = options;
  const initial = await client.service.getSnapshot();
  if (!initial.ok) throw new Error(initial.error.message);
  const stateFile = path.join(userData, 'desktop-window.json');
  let saved = null;
  try { const raw = readFileSync(stateFile, 'utf8'); if (raw.length < 4096) saved = parseWindowState(JSON.parse(raw)); } catch { /* First client window uses default bounds. */ }
  const bounds = restoreBounds(saved, screen.getAllDisplays().map(display => display.workArea), screen.getPrimaryDisplay().workArea);
  const rendererFile = path.resolve(__dirname, '../renderer/index.html'), rendererUrl = pathToFileURL(rendererFile).href;
  installDesktopPolicy();
  const color = initial.value.settings.backgroundColor;
  const window = new BrowserWindow({ ...bounds.options,
    ...(backgroundTest ? { show: false, focusable: false, skipTaskbar: true } : {}),
    icon: path.resolve(__dirname, '../icon', process.platform === 'win32' ? 'icon.ico' : 'icon-256.png'),
    minWidth: 850, minHeight: 600, backgroundColor: /^#[0-9a-fA-F]{6}$/.test(color) ? color : '#111016',
    title: `Shellfox v${app.getVersion()} · host client`,
    webPreferences: { ...(backgroundTest ? { backgroundThrottling: false } : {}), preload: path.resolve(__dirname, '../preload/index.cjs'), contextIsolation: true, nodeIntegration: false, sandbox: true, webSecurity: true },
  });
  secureDesktopWindow(window);
  window.on('page-title-updated', event => event.preventDefault());
  if (bounds.maximized && !backgroundTest) window.maximize();
  const tracker = trackWindowState(window, state => {
    writeFileSync(stateFile + '.new', JSON.stringify(state), { mode: 0o600 }); renameSync(stateFile + '.new', stateFile);
  });
  const confirmation = new CloseConfirmationCoordinator(window, rendererUrl, { autoApprove: options.confirmationAutoApprove, present: () => {
    if (window.isDestroyed() || backgroundTest) return;
    if (window.isMinimized()) window.restore(); window.show(); window.focus();
  } });
  let quitting = false, closing = false;
  let uninstall: () => Promise<void> = async () => {};
  let updates: UpdateSource | undefined;
  async function finish(): Promise<void> {
    quitting = true;
    await uninstall(); confirmation.dispose(); tracker.dispose();
    await client.close();
    if (updates instanceof SelfUpdater) await updates.dispose(true);
    app.quit();
  }
  const shutdown = new UpdateShutdown({ confirm: onQuit => onQuit || confirmation.request('update', 0), closeTerminals: async () => () => {}, finish });
  updates = __TEST_BUILD__
    ? new UpdateChecker({ current: app.getVersion(), check: async () => process.env.SHELLFOX_TEST_LATEST_RELEASE ?? null })
    : new SelfUpdater({ current: app.getVersion(), platform: await createUpdatePlatform({ platform: process.platform, arch: process.arch, executable: process.execPath, packaged: app.isPackaged }),
      fetch: (url, init) => net.fetch(url, init), tempRoot: app.getPath('temp'),
      autoUpdate: async () => {
        const snapshot = await client.service.getSnapshot();
        return snapshot.ok && snapshot.value.settings.autoUpdate;
      },
      quitForUpdate: async (start, onQuit) => {
        if (quitting || closing && !onQuit) return false;
        return shutdown.run(start, onQuit);
      },
    });
  const adapter = {
    createSession: client.service.createSession,
    selectSession: (id: string) => { void client.service.getSnapshot().then(snapshot => {
      if (snapshot.ok) sendToRenderer(window, rendererUrl, 'manager:changed', { revision: snapshot.value.revision, reason: 'sessions', selectSessionId: id });
    }).catch(() => {}); },
  };
  uninstall = installAttachedHostIpc(window, rendererUrl, client, () => requests.ready(async request => {
    const result = await handleCliRequest(request, adapter, window, !backgroundTest);
    if (!result.ok) dialog.showErrorBox('Could not create session', result.error.message);
  }), updates);
  try { await window.loadFile(rendererFile); }
  catch (error) { await uninstall(); confirmation.dispose(); tracker.dispose(); await client.close(); if (updates instanceof SelfUpdater) await updates.dispose(); throw error; }
  if (updates instanceof SelfUpdater && app.isPackaged) updates.start();
  window.on('close', event => { if (!quitting) { event.preventDefault(); app.quit(); } });
  app.on('before-quit', event => {
    if (quitting) return;
    event.preventDefault(); if (closing) return; closing = true;
    void (async () => {
      if (updates instanceof SelfUpdater && await updates.installOnQuit() !== 'not-ready') return;
      if (updates instanceof SelfUpdater) await updates.dispose();
      await finish();
    })().catch(() => { closing = false; dialog.showErrorBox('Shutdown failed', 'The host client could not disconnect. Its host terminals were not stopped.'); });
  });
  app.on('window-all-closed', () => app.quit());
}
