import { app, BrowserWindow, dialog, Menu, net, powerSaveBlocker, screen } from 'electron';
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
import { bypassCloseConfirmation, CloseConfirmationCoordinator } from './close-confirmation';
import { installIpc } from './ipc';
import { desktopRequest } from './desktop-requests';
import { installDesktopPolicy, secureDesktopWindow } from './desktop-security';
import { runAttachedDesktop } from './attached-desktop';
import { onSystemQuit } from './system-quit';
import { acquireHostOwnership } from '../host/ownership';
import { connectHostClient } from '../host/client';
import { startHostRpc, type HostRpc } from '../host/rpc';
import { RemoteService } from './remote/service';
import { RemoteStore } from './remote/storage';
import { RemoteHelper } from './remote/helper';
import { installRemoteIpc } from './remote/ipc';
import { hostname as systemHostname } from 'node:os';
import { KeepAwake } from './keep-awake';
import { startControlServer } from './control-server';
import { helpText } from './ssh/presentation';
import { SshProfileStore } from './ssh/storage';
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
// Startup can fail after the listener has published credentials. Normal quit
// owns these resources only after run() finishes installing its lifecycle hooks.
const startupCleanups: Array<() => void | Promise<void>> = [];
async function run(): Promise<void> {
  if (lifecycle) {
    try {
      await migrateUserData(appData, productData);
      await handleEmbeddedInstaller(lifecycle, explorerIntegration(), new WindowsCliIntegration({ executable: process.execPath, updateScript: windowsUpdateScript })); app.exit(0);
    }
    catch { app.exit(1); }
    return;
  }
  if (args.includes('--help')) { process.stdout.write(helpText(app.getVersion(), process.stdout.isTTY === true && process.env.NO_COLOR === undefined, process.platform !== 'win32')); app.exit(0); return; }
  const parsed = parseCli(args.filter(a => a !== '--squirrel-firstrun'), { testMode: process.env.SHELLFOX_TEST_MODE === '1', testBuild: __TEST_BUILD__, projectRoot: __TEST_BUILD__ ? __PROJECT_ROOT__ : process.env.SHELLFOX_TEST_ROOT });
  if (!parsed.ok) { console.error('Shellfox: ' + parsed.error.message); app.exit(1); return; }
  if (parsed.value.userData) app.setPath('userData', parsed.value.userData);
  // Resolve/check in the invoking process, before forwarding to the running app.
  if (parsed.value.request.kind === 'new-session') {
    try { parsed.value.request.cwd = await validateDirectory(parsed.value.request.cwd); }
    catch { console.error('Shellfox: directory does not exist or is not accessible.'); app.exit(1); return; }
  }
  const cliOptions = {
    resourcesPath: app.isPackaged ? process.resourcesPath : undefined,
    projectRoot: !app.isPackaged ? (__TEST_BUILD__ ? __PROJECT_ROOT__ : path.resolve(__dirname, '../../..')) : undefined,
    userData: app.getPath('userData'), cliHelper: app.isPackaged ? path.join(process.resourcesPath, 'cli/main/cli.cjs') : path.join(__dirname, '../cli/main/cli.cjs'),
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
  Menu.setApplicationMenu(null);
  await mkdir(app.getPath('userData'), { recursive: true });
  const ownership = await acquireHostOwnership(app.getPath('userData'), 'desktop', { resourcesPath: app.isPackaged ? process.resourcesPath : undefined, packaged: app.isPackaged });
  if (!ownership) {
    const client = await connectHostClient(app.getPath('userData'));
    startupCleanups.push(() => client.close());
    // Desktop integration belongs to this UI installation, not the headless host's database.
    const integrationResults = await Promise.allSettled([cliIntegration.set(true), folderIntegration.set(true)]);
    for (const result of integrationResults) {
      if (result.status === 'rejected') console.error('Automatic desktop integration could not be enabled.');
      else if (!result.value.ok && result.value.error.code !== 'UNSUPPORTED') console.error('Automatic desktop integration:', result.value.error.message);
    }
    await runAttachedDesktop({ client, requests, userData: app.getPath('userData'),
      backgroundTest: !!parsed.value.userData && process.env.SHELLFOX_TEST_MODE === '1' && process.env.SHELLFOX_TEST_VISIBLE !== '1',
      confirmationAutoApprove: bypassCloseConfirmation(__TEST_BUILD__, parsed.value, process.env.SHELLFOX_TEST_CONFIRM_QUIT),
    });
    startupCleanups.length = 0; return;
  }
  let failOwnership = async (error: Error) => { console.error('Host ownership lost:', error.message); app.exit(1); };
  const stopOwnershipWatch = ownership.onLost(error => { void failOwnership(error); });
  startupCleanups.push(() => { stopOwnershipWatch(); return ownership.release(); });
  ownership.assertOwned();
  if (!parsed.value.userData || __TEST_BUILD__ && parsed.value.userData === productData) await migrateUserData(appData, productData);
  const repository = new Repository(path.join(app.getPath('userData'), 'manager.sqlite3'));
  startupCleanups.push(() => repository.close());
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
  startupCleanups.push(() => service.dispose());
  await service.initialize(nativePaths()); ownership.assertOwned();
  const remoteHostname = ('shellfox-' + systemHostname().toLowerCase().replace(/[^a-z0-9-]/g, '-')).slice(0, 63).replace(/-+$/, '') || 'shellfox';
  const remoteHelperName = 'shellfox-tailscale' + (process.platform === 'win32' ? '.exe' : '');
  const testRemoteHelper = __TEST_BUILD__ && process.env.SHELLFOX_TEST_REMOTE_HELPER && path.resolve(process.env.SHELLFOX_TEST_REMOTE_HELPER).startsWith(path.join(__PROJECT_ROOT__, 'tmp') + path.sep)
    ? path.resolve(process.env.SHELLFOX_TEST_REMOTE_HELPER) : null;
  const remoteHelperPath = testRemoteHelper ?? (app.isPackaged ? path.join(process.resourcesPath, 'tailscale', remoteHelperName) : path.resolve('tmp/tailscale', process.platform + '-' + process.arch, remoteHelperName));
  const remote = service instanceof EmbeddedSessionService ? new RemoteService(app.getPath('userData'), new RemoteStore(app.getPath('userData'), remoteHostname), new RemoteHelper(remoteHelperPath), service, remoteHostname) : null;
  if (remote) { startupCleanups.push(() => remote.dispose()); await remote.initialize(); }
  const keepAwake = new KeepAwake(powerSaveBlocker, { stateDir: app.getPath('userData') });
  startupCleanups.push(() => keepAwake.dispose());
  app.on('will-quit', () => keepAwake.dispose());
  const controlServer = await startControlServer(app.getPath('userData'), service, keepAwake, () => ownership.assertOwned());
  startupCleanups.push(() => controlServer.dispose());
  let hostRpc: HostRpc | undefined;
  let fatal = false;
  failOwnership = async error => {
    if (fatal) return; fatal = true;
    console.error('Host ownership lost:', error.message);
    try {
      const terminals = service instanceof EmbeddedSessionService ? service.dispose({ fatal: true }) : service.dispose();
      await Promise.allSettled([terminals, hostRpc?.close(), controlServer.dispose(), remote?.dispose()]);
      repository.close();
    } finally { await ownership.release().catch(() => {}); app.exit(1); }
  };
  ownership.assertOwned();
  const rendererFile = path.resolve(__dirname, '../renderer/index.html');
  const rendererUrl = pathToFileURL(rendererFile).href;
  installDesktopPolicy();
  let savedWindowState: ReturnType<typeof parseWindowState> = null;
  try { savedWindowState = parseWindowState(repository.windowState()); } catch { /* fall back to defaults */ }
  const restored = (() => {
    try { return restoreBounds(savedWindowState, screen.getAllDisplays().map(d => d.workArea), screen.getPrimaryDisplay().workArea); }
    catch { return restoreBounds(null, [], { x: 0, y: 0, width: 1220, height: 820 }); }
  })();
  // Paint the saved theme background before the renderer loads (no flash on light themes).
  let windowBackground = '#111016';
  try { const saved = repository.settings().backgroundColor; if (/^#[0-9a-fA-F]{6}$/.test(saved)) windowBackground = saved; } catch { /* default */ }
  // Only validated, isolated test launches can suppress native window presentation.
  // Keep rendering active so Playwright can exercise the real UI without stealing focus.
  const backgroundTest = !!parsed.value.userData && process.env.SHELLFOX_TEST_MODE === '1' && process.env.SHELLFOX_TEST_VISIBLE !== '1';
  window = new BrowserWindow({
    ...restored.options,
    ...(backgroundTest ? { show: false, focusable: false, skipTaskbar: true } : {}),
    icon: path.resolve(__dirname, '../icon', process.platform === 'win32' ? 'icon.ico' : 'icon-256.png'),
    minWidth: 850, minHeight: 600, backgroundColor: windowBackground, title: `Shellfox v${app.getVersion()}`,
    webPreferences: { ...(backgroundTest ? { backgroundThrottling: false } : {}), preload: path.resolve(__dirname, '../preload/index.cjs'), contextIsolation: true, nodeIntegration: false, sandbox: true, webSecurity: true },
  });
  if (restored.maximized && !backgroundTest) window.maximize();
  const windowTracker = trackWindowState(window, state => repository.saveWindowState(state));
  startupCleanups.push(() => windowTracker.dispose());
  secureDesktopWindow(window);
  const ownedTerminalCount = () => 'ownedTerminalCount' in service ? service.ownedTerminalCount() : 0;
  const closeConfirmation = new CloseConfirmationCoordinator(window, rendererUrl, {
    autoApprove: bypassCloseConfirmation(__TEST_BUILD__, parsed.value, process.env.SHELLFOX_TEST_CONFIRM_QUIT),
    present: () => {
      if (!window || window.isDestroyed() || backgroundTest) return;
      if (window.isMinimized()) window.restore();
      window.show(); window.focus();
    },
  });
  startupCleanups.push(() => closeConfirmation.dispose());
  let quitting = false;
  let closingForUpdate = false;
  let closingForQuit = false;
  let quitApproved = false;
  let cliStop: { resolve(): void; reject(error: Error): void } | undefined;
  const updateShutdown = new UpdateShutdown({
    confirm: onQuit => {
      if (onQuit && !ownedTerminalCount()) return true;
      return closeConfirmation.request('update', ownedTerminalCount());
    },
    closeTerminals: () => {
      if (!(service instanceof EmbeddedSessionService)) throw new Error('Update shutdown requires embedded terminals.');
      return service.closeForUpdate();
    },
    finish: async () => {
      quitting = true;
      // Commit is acknowledged. Never retry installation because final cleanup failed.
      try {
        await remote?.dispose();
        await hostRpc?.close();
        await controlServer.dispose().catch(error => console.error('Control shutdown cleanup failed:', error));
        await service.dispose();
        if (updates instanceof SelfUpdater) await updates.dispose(true);
        uninstallIpc(); windowTracker.dispose(); repository.close(); await ownership.release();
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
      quitForUpdate: async (start, onQuit) => {
        if (quitting || closingForUpdate || closingForQuit && !onQuit) return false;
        closingForUpdate = true;
        try { return await updateShutdown.run(start, onQuit); }
        finally { closingForUpdate = false; }
      },
    });
  if (updates instanceof SelfUpdater) startupCleanups.push(() => updates.dispose());
  if (updates instanceof SelfUpdater && app.isPackaged) updates.start();
  const sshProfiles = new SshProfileStore(app.getPath('userData'));
  const uninstallManagerIpc = installIpc(window, rendererUrl, service, () => requests.ready(async request => {
    if (!window || window.isDestroyed()) return;
    const result = await handleCliRequest(request, service, window, !backgroundTest);
    if (!result.ok) dialog.showErrorBox('Could not create session', result.error.message);
  }), updates, sshProfiles, keepAwake, () => ownership.assertOwned());
  const uninstallRemoteIpc = installRemoteIpc(window, rendererUrl, remote);
  const uninstallIpc = () => { closeConfirmation.dispose(); uninstallRemoteIpc(); uninstallManagerIpc(); };
  startupCleanups.push(uninstallIpc);
  const quitGuard = new TerminalQuitGuard({ ownedTerminalCount, dispose: () => service.dispose() }, count => quitApproved || closeConfirmation.request('quit', count));
  // The CLI stop command and the OS approve quitting themselves; never prompt or install an update then.
  const uninstallSystemQuit = onSystemQuit(window, () => { quitApproved = true; closeConfirmation.settleForSystemQuit(); setImmediate(() => app.quit()); });
  startupCleanups.push(uninstallSystemQuit);
  window.on('close', event => {
    if (quitting) return;
    event.preventDefault(); app.quit();
  });
  app.on('before-quit', event => {
    if (quitting) return;
    event.preventDefault();
    if (closingForUpdate || closingForQuit) return;
    closingForQuit = true;
    void updateShutdown.cancelPending().then(async () => {
      // Use the verified handoff before normal quit disposes the service and
      // deletes staged files. Cancellation or failure must not fall through.
      if (!quitApproved && updates instanceof SelfUpdater && await updates.installOnQuit() !== 'not-ready') return false;
      return quitGuard.request();
    }).then(async approved => {
      if (!approved || quitting) { closingForQuit = false; return; }
      quitting = true;
      // Terminal disposal succeeded. Non-critical cleanup must not strand an
      // open manager whose backend has already been irreversibly disposed.
      try {
        await remote?.dispose();
        await hostRpc?.close();
        await controlServer.dispose().catch(error => console.error('Control shutdown cleanup failed:', error));
        if (updates instanceof SelfUpdater) await updates.dispose();
        uninstallIpc(); windowTracker.dispose(); repository.close(); await ownership.release();
      } catch (error) { console.error('Quit cleanup failed:', error); }
      cliStop?.resolve(); cliStop = undefined; quitApproved = false;
      await hostRpc?.waitForStopReplies();
      app.quit();
    }).catch(() => {
      closingForQuit = false; quitting = false; quitApproved = false;
      cliStop?.reject(new Error('Owned terminal shutdown failed.')); cliStop = undefined;
      dialog.showErrorBox('Shutdown failed', 'The update or owned terminal shutdown could not finish safely. Shellfox is still open. Closed terminals must be reopened as fresh shells.');
    });
  });
  app.on('window-all-closed', () => app.quit());
  if (service instanceof EmbeddedSessionService) {
    const hostWindow = window;
    hostRpc = await startHostRpc({ userData: app.getPath('userData'), service, remote, keepAwake, ssh: sshProfiles, mode: 'desktop', guard: () => ownership.assertOwned(),
      desktop: (method, payload) => desktopRequest(method, payload, { window: hostWindow, updates, sessionDirectory: id => service.repository.session(id)?.cwd ?? null }),
      shutdown: () => new Promise<void>((resolve, reject) => { cliStop = { resolve, reject }; quitApproved = true; app.quit(); }),
    });
    startupCleanups.push(() => hostRpc?.close());
  }
  // A visible desktop is also a ready CLI host, not a partially published endpoint.
  await window.loadFile(rendererFile);
  startupCleanups.length = 0;
}
void run().catch(async error => {
  for (const cleanup of startupCleanups.splice(0).reverse()) {
    try { await cleanup(); } catch (cleanupError) { console.error('Startup shutdown cleanup failed:', cleanupError); }
  }
  console.error('Shellfox startup failed:', error);
  dialog.showErrorBox('Shellfox', 'The manager could not start. Check the installed application and data directory.'); app.exit(1);
});
