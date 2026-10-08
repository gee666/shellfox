import { app, BrowserWindow, dialog, Menu, net, screen, session } from 'electron';
import { parseWindowState, restoreBounds, trackWindowState } from './window-state';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
import { validateDirectory } from './directory';
import { migrateUserData } from './user-data-upgrade';
import { LinuxCliIntegration } from './platform/linux-cli';
import { LinuxFileMenus } from './platform/linux-menus';
import { WindowsCliIntegration } from './platform/shellfox-cli';
import { mkdir } from 'node:fs/promises';
import type { NativeBackend, NativeInit } from '../shared/native-port';
import { EarlyRequestQueue, handleCliRequest, parseCli, productArguments } from './cli';
import { WindowsExplorer } from './platform/explorer';
import { Repository } from './repository';
import { SessionService } from './service';
import { EmbeddedSessionService } from './terminal/service';
import { TerminalQuitGuard } from './terminal/shutdown';
import { installIpc } from './ipc';
import { handleEmbeddedInstaller, installerEvent } from './installer';
import { UpdateChecker } from './update-check';
import { SelfUpdater, type UpdateSource } from './update/self-updater';
import { createUpdatePlatform } from './update/platform-installer';
import { UpdateShutdown } from './update/update-shutdown';
import windowsUpdateScript from './update/shellfox-update.ps1';
declare const __TEST_BUILD__: boolean;
declare const __PROJECT_ROOT__: string;
app.setName('Shellfox');
// Stable for all versions and Squirrel lifecycle invocations.
const appData = __TEST_BUILD__ && process.env.SHELLFOX_TEST_APP_DATA && path.resolve(process.env.SHELLFOX_TEST_APP_DATA).startsWith(path.join(__PROJECT_ROOT__, 'tmp') + path.sep)
  ? path.resolve(process.env.SHELLFOX_TEST_APP_DATA) : app.getPath('appData');
const productData = path.join(appData, 'Shellfox');
if (process.platform === 'win32') app.setAppUserModelId('com.squirrel.shellfox.Shellfox');
app.setPath('userData', productData);
const args = productArguments(process.argv.slice(app.isPackaged ? 1 : 2));
const lifecycle = installerEvent(args);
const explorerIntegration = (isolatedData?: string) => {
  const testKey = isolatedData ? 'Software\\Shellfox\\Tests\\' + createHash('sha256').update(isolatedData).digest('hex') + '\\Explorer' : null;
  return new WindowsExplorer({ executable: process.execPath, ...(app.isPackaged ? {} : { appPath: path.join(__dirname, 'index.cjs') }),
    ...(testKey ? { keys: [testKey + '\\Folder', testKey + '\\Background'], legacyKeys: [testKey + '\\LegacyFolder', testKey + '\\LegacyBackground'] } : {}) });
};
const nativePaths = (): NativeInit => ({
  userDataDir: app.getPath('userData'),
  helperDir: app.isPackaged ? path.join(process.resourcesPath, 'native') : path.resolve(process.platform === 'linux' ? 'tmp/native/linux-x64' : 'tmp/native/win-x64'),
  shellScriptDir: __TEST_BUILD__ && process.env.SHELLFOX_TEST_SHELL_DIR && path.resolve(process.env.SHELLFOX_TEST_SHELL_DIR).startsWith(path.join(__PROJECT_ROOT__, 'tmp') + path.sep)
    ? path.resolve(process.env.SHELLFOX_TEST_SHELL_DIR)
    : app.isPackaged ? path.join(process.resourcesPath, 'shell') : path.resolve(process.platform === 'linux' ? 'resources/shell-linux' : 'resources/shell'),
  packagedExecutable: app.isPackaged ? process.execPath : null,
});
async function run(): Promise<void> {
  if (lifecycle) {
    try {
      await migrateUserData(appData, productData);
      await handleEmbeddedInstaller(lifecycle, explorerIntegration(), new WindowsCliIntegration({ executable: process.execPath, updateScript: windowsUpdateScript })); app.exit(0);
    }
    catch { app.exit(1); }
    return;
  }
  if (args.includes('--help')) { console.log('Usage: shellfox start [path]'); app.exit(0); return; }
  const parsed = parseCli(args.filter(a => a !== '--squirrel-firstrun'), { testMode: process.env.SHELLFOX_TEST_MODE === '1', testBuild: __TEST_BUILD__, projectRoot: __TEST_BUILD__ ? __PROJECT_ROOT__ : process.env.SHELLFOX_TEST_ROOT });
  if (!parsed.ok) { console.error('Shellfox: ' + parsed.error.message); app.exit(1); return; }
  if (parsed.value.userData) app.setPath('userData', parsed.value.userData);
  // Resolve/check in the invoking process, before forwarding to the running app.
  if (parsed.value.request.kind === 'new-session') {
    try { parsed.value.request.cwd = await validateDirectory(parsed.value.request.cwd); }
    catch { console.error('Shellfox: directory does not exist or is not accessible.'); app.exit(1); return; }
  }
  const cliOptions = {
    updateScript: windowsUpdateScript, executable: process.execPath, ...(app.isPackaged ? {} : { appPath: path.join(__dirname, 'index.cjs') }),
    ...(parsed.value.userData ? {
      binDir: path.join(parsed.value.userData, 'Shellfox', 'bin'),
      registryKey: 'Software\\Shellfox\\Tests\\' + createHash('sha256').update(parsed.value.userData).digest('hex') + '\\Environment',
      prefixArgs: ['--test-user-data', parsed.value.userData, '--test-backend', parsed.value.backend!],
      launchEnv: { SHELLFOX_TEST_MODE: '1', SHELLFOX_TEST_ROOT: __TEST_BUILD__ ? __PROJECT_ROOT__ : process.env.SHELLFOX_TEST_ROOT! },
    } : {}),
  };
  const cliIntegration = process.platform === 'linux' || process.platform === 'darwin'
    ? new LinuxCliIntegration({ ...cliOptions, ...(parsed.value.userData ? { home: parsed.value.userData } : {}) })
    : new WindowsCliIntegration(cliOptions);
  const folderIntegration = process.platform === 'linux' && cliIntegration instanceof LinuxCliIntegration
    ? new LinuxFileMenus(cliIntegration, parsed.value.userData ? { home: parsed.value.userData, env: { ...process.env, XDG_DATA_HOME: path.join(parsed.value.userData, '.local/share'), XDG_CONFIG_HOME: path.join(parsed.value.userData, '.config') } } : {})
    : explorerIntegration(parsed.value.userData);
  const requests = new EarlyRequestQueue();
  if (!app.requestSingleInstanceLock(parsed.value.request)) { app.exit(0); return; }
  requests.enqueue(parsed.value.request);
  let window: BrowserWindow | undefined;
  app.on('second-instance', (_event, _argv, _cwd, additionalData) => {
    const result = requests.enqueue(additionalData);
    if (!result.ok) dialog.showErrorBox('Shellfox', result.error.message);
  });
  await app.whenReady();
  if (process.platform === 'win32') Menu.setApplicationMenu(null);
  await mkdir(app.getPath('userData'), { recursive: true });
  if (!parsed.value.userData || __TEST_BUILD__ && parsed.value.userData === productData) await migrateUserData(appData, productData);
  const repository = new Repository(path.join(app.getPath('userData'), 'manager.sqlite3'));
  let service: SessionService | EmbeddedSessionService;
  if (__TEST_BUILD__ && parsed.value.backend === 'fake') {
    const { createFakeNativeBackend } = await import('#test-native-backend');
    let backend: NativeBackend;
    if (process.env.SHELLFOX_FAKE_SESSION_WINDOWS === '1') {
      const { WindowTestBackend } = await import('./window-test-backend');backend = new WindowTestBackend();
    } else backend = createFakeNativeBackend();
    service = new SessionService(repository, backend, undefined, app.isPackaged ? process.execPath : null);
  } else service = new EmbeddedSessionService(repository, undefined, undefined, folderIntegration, cliIntegration);
  if (__TEST_BUILD__) (globalThis as any).__shellfoxTest = { service, repository, backend: service.backend };
  await service.initialize(nativePaths());
  const rendererFile = path.resolve(__dirname, '../renderer/index.html');
  const rendererUrl = pathToFileURL(rendererFile).href;
  const csp = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'none'; base-uri 'none'; form-action 'none'; frame-src 'none'; object-src 'none'";
  session.defaultSession.webRequest.onHeadersReceived((details, callback) => callback({ responseHeaders: { ...details.responseHeaders, 'Content-Security-Policy': [csp] } }));
  session.defaultSession.setPermissionRequestHandler((_wc, _permission, callback) => callback(false));
  session.defaultSession.setPermissionCheckHandler(() => false);
  let savedWindowState: ReturnType<typeof parseWindowState> = null;
  try { savedWindowState = parseWindowState(repository.windowState()); } catch { /* fall back to defaults */ }
  const restored = (() => {
    try { return restoreBounds(savedWindowState, screen.getAllDisplays().map(d => d.workArea), screen.getPrimaryDisplay().workArea); }
    catch { return restoreBounds(null, [], { x: 0, y: 0, width: 1220, height: 820 }); }
  })();
  // Paint the saved theme background before the renderer loads (no flash on light themes).
  let windowBackground = '#111016';
  try { const saved = repository.settings().backgroundColor; if (/^#[0-9a-fA-F]{6}$/.test(saved)) windowBackground = saved; } catch { /* default */ }
  window = new BrowserWindow({
    ...restored.options,
    icon: path.resolve(__dirname, '../icon', process.platform === 'win32' ? 'icon.ico' : 'icon-256.png'),
    minWidth: 850, minHeight: 600, backgroundColor: windowBackground, title: `Shellfox v${app.getVersion()}`,
    webPreferences: { preload: path.resolve(__dirname, '../preload/index.cjs'), contextIsolation: true, nodeIntegration: false, sandbox: true, webSecurity: true },
  });
  if (restored.maximized) window.maximize();
  const windowTracker = trackWindowState(window, state => repository.saveWindowState(state));
  window.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  window.webContents.on('will-navigate', event => event.preventDefault());
  window.webContents.on('will-redirect', event => event.preventDefault());
  window.webContents.on('will-attach-webview', event => event.preventDefault());
  let quitting = false;
  let closingForUpdate = false;
  let closingForQuit = false;
  const updateShutdown = new UpdateShutdown({
    confirm: () => {
      const options = { type: 'warning' as const, title: 'Install update and close all terminals?',
        message: 'Close all Shellfox terminals and install the update?',
        detail: 'All embedded shells and their running commands will stop. Unsaved work may be lost. Shellfox will restart with fresh shells, not restored commands. Legacy external terminals are not touched. The installer may ask for administrator approval.',
        buttons: ['Cancel', 'Close all terminals and install'], defaultId: 0, cancelId: 0, noLink: true };
      return (window && !window.isDestroyed() ? dialog.showMessageBoxSync(window, options) : dialog.showMessageBoxSync(options)) === 1;
    },
    closeTerminals: () => {
      if (!(service instanceof EmbeddedSessionService)) throw new Error('Update shutdown requires embedded terminals.');
      return service.closeForUpdate();
    },
    finish: async () => {
      quitting = true;
      // Commit is acknowledged. Never retry installation because final cleanup failed.
      try {
        await service.dispose();
        if (updates instanceof SelfUpdater) await updates.dispose(true);
        uninstallIpc(); windowTracker.dispose(); repository.close();
      } catch (error) { console.error('Update shutdown cleanup failed:', error); }
      app.quit();
    },
  });
  // Tests never contact GitHub or launch installers. Development builds report unsupported.
  const updates: UpdateSource = __TEST_BUILD__
    ? new UpdateChecker({ current: app.getVersion(), check: async () => process.env.SHELLFOX_TEST_LATEST_RELEASE ?? null })
    : new SelfUpdater({ current: app.getVersion(),
      platform: await createUpdatePlatform({ platform: process.platform, arch: process.arch, executable: process.execPath, packaged: app.isPackaged }),
      fetch: (url, init) => net.fetch(url, init), tempRoot: app.getPath('temp'),
      quitForUpdate: async start => {
        if (quitting || closingForUpdate || closingForQuit) return false;
        closingForUpdate = true;
        try { return await updateShutdown.run(start); }
        finally { closingForUpdate = false; }
      },
    });
  if (updates instanceof SelfUpdater && app.isPackaged) updates.start();
  const uninstallIpc = installIpc(window, rendererUrl, service, () => requests.ready(async request => {
    if (!window || window.isDestroyed()) return;
    const result = await handleCliRequest(request, service, window);
    if (!result.ok) dialog.showErrorBox('Could not create session', result.error.message);
  }), updates);
  await window.loadFile(rendererFile);
  const quitGuard = new TerminalQuitGuard({ ownedTerminalCount: () => 'ownedTerminalCount' in service ? service.ownedTerminalCount() : 0, dispose: () => service.dispose() }, count => {
    const options = { type: 'warning' as const, title: 'Close embedded shells and quit?', message: `Quit Shellfox and close ${count} owned terminal${count === 1 ? '' : 's'}?`, detail: 'Embedded shells do not survive manager shutdown. Running commands may be interrupted. Restarting opens fresh shells; it does not restore commands. Legacy external terminals are not touched.', buttons: ['Cancel', 'Close shells and quit'], defaultId: 0, cancelId: 0, noLink: true };
    return (window && !window.isDestroyed() ? dialog.showMessageBoxSync(window, options) : dialog.showMessageBoxSync(options)) === 1;
  });
  window.on('close', event => {
    if (quitting) return;
    event.preventDefault(); app.quit();
  });
  app.on('before-quit', event => {
    if (quitting) return;
    event.preventDefault();
    if (closingForUpdate || closingForQuit) return;
    closingForQuit = true;
    void updateShutdown.cancelPending().then(() => quitGuard.request()).then(async approved => {
      if (!approved || quitting) { closingForQuit = false; return; }
      quitting = true;
      // Terminal disposal succeeded. Non-critical cleanup must not strand an
      // open manager whose backend has already been irreversibly disposed.
      try {
        if (updates instanceof SelfUpdater) await updates.dispose();
        uninstallIpc(); windowTracker.dispose(); repository.close();
      } catch (error) { console.error('Quit cleanup failed:', error); }
      app.quit();
    }).catch(() => { closingForQuit = false; quitting = false; dialog.showErrorBox('Shutdown failed', 'Owned terminals could not be closed cleanly. Shellfox has not claimed shell survival.'); });
  });
  app.on('window-all-closed', () => app.quit());
}
void run().catch(error => { console.error('Shellfox startup failed:', error); dialog.showErrorBox('Shellfox', 'The manager could not start. Check the installed application and data directory.'); app.exit(1); });
