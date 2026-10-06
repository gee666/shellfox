import { app, BrowserWindow, dialog, session } from 'electron';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
import { validateDirectory } from './directory';
import { migrateUserData } from './user-data-upgrade';
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
      await handleEmbeddedInstaller(lifecycle, explorerIntegration(), new WindowsCliIntegration({ executable: process.execPath })); app.exit(0);
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
  const cliIntegration = new WindowsCliIntegration({
    executable: process.execPath, ...(app.isPackaged ? {} : { appPath: path.join(__dirname, 'index.cjs') }),
    ...(parsed.value.userData ? {
      binDir: path.join(parsed.value.userData, 'Shellfox', 'bin'),
      registryKey: 'Software\\Shellfox\\Tests\\' + createHash('sha256').update(parsed.value.userData).digest('hex') + '\\Environment',
      prefixArgs: ['--test-user-data', parsed.value.userData, '--test-backend', parsed.value.backend!],
      launchEnv: { SHELLFOX_TEST_MODE: '1', SHELLFOX_TEST_ROOT: __TEST_BUILD__ ? __PROJECT_ROOT__ : process.env.SHELLFOX_TEST_ROOT! },
    } : {}),
  });
  const requests = new EarlyRequestQueue();
  if (!app.requestSingleInstanceLock(parsed.value.request)) { app.exit(0); return; }
  requests.enqueue(parsed.value.request);
  let window: BrowserWindow | undefined;
  app.on('second-instance', (_event, _argv, _cwd, additionalData) => {
    const result = requests.enqueue(additionalData);
    if (!result.ok) dialog.showErrorBox('Shellfox', result.error.message);
  });
  await app.whenReady();
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
  } else service = new EmbeddedSessionService(repository, undefined, undefined, explorerIntegration(parsed.value.userData), cliIntegration);
  if (__TEST_BUILD__) (globalThis as any).__shellfoxTest = { service, repository, backend: service.backend };
  await service.initialize(nativePaths());
  const rendererFile = path.resolve(__dirname, '../renderer/index.html');
  const rendererUrl = pathToFileURL(rendererFile).href;
  const csp = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'none'; base-uri 'none'; form-action 'none'; frame-src 'none'; object-src 'none'";
  session.defaultSession.webRequest.onHeadersReceived((details, callback) => callback({ responseHeaders: { ...details.responseHeaders, 'Content-Security-Policy': [csp] } }));
  session.defaultSession.setPermissionRequestHandler((_wc, _permission, callback) => callback(false));
  session.defaultSession.setPermissionCheckHandler(() => false);
  window = new BrowserWindow({
    width: 1220, height: 820, minWidth: 850, minHeight: 600, backgroundColor: '#111118', title: 'Shellfox',
    webPreferences: { preload: path.resolve(__dirname, '../preload/index.cjs'), contextIsolation: true, nodeIntegration: false, sandbox: true, webSecurity: true },
  });
  window.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  window.webContents.on('will-navigate', event => event.preventDefault());
  window.webContents.on('will-redirect', event => event.preventDefault());
  window.webContents.on('will-attach-webview', event => event.preventDefault());
  const uninstallIpc = installIpc(window, rendererUrl, service, () => requests.ready(async request => {
    if (!window || window.isDestroyed()) return;
    const result = await handleCliRequest(request, service, window);
    if (!result.ok) dialog.showErrorBox('Could not create session', result.error.message);
  }));
  await window.loadFile(rendererFile);
  let quitting = false;
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
    void quitGuard.request().then(approved => {
      if (!approved || quitting) return;
      quitting = true; uninstallIpc(); repository.close(); app.quit();
    }).catch(() => dialog.showErrorBox('Shutdown failed', 'Owned terminals could not be closed cleanly. Shellfox has not claimed shell survival.'));
  });
  app.on('window-all-closed', () => app.quit());
}
void run().catch(() => { dialog.showErrorBox('Shellfox', 'The manager could not start. Check the installed application and data directory.'); app.exit(1); });
