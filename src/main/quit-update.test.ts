import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readdir, rm } from 'node:fs/promises';
import path from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { SelfUpdater } from './update/self-updater';

// Exercise the actual main close/before-quit wiring and updater transaction.
// Only Electron, storage and terminal/installer effects are replaced.
const state = vi.hoisted(() => ({
  root: '', userData: '', owned: 0, committed: false, updater: undefined as SelfUpdater | undefined,
  app: {
    isPackaged: true, setName: vi.fn(), setAppUserModelId: vi.fn(),
    getVersion: () => '0.2.5', getPath: vi.fn(), setPath: vi.fn(),
    requestSingleInstanceLock: () => true, whenReady: async () => {},
    on: vi.fn(), quit: vi.fn(), exit: vi.fn(),
  },
  window: {
    on: vi.fn(), isDestroyed: () => false, loadFile: async () => {},
    webContents: { on: vi.fn(), setWindowOpenHandler: vi.fn() },
  },
  dialog: { showMessageBoxSync: vi.fn(), showErrorBox: vi.fn() },
  service: { initialize: async () => {}, ownedTerminalCount: vi.fn(), closeForUpdate: vi.fn(), dispose: vi.fn() },
  repository: { windowState: () => null, settings: () => ({}), close: vi.fn() },
  fetch: vi.fn(), prepare: vi.fn(), start: vi.fn(), commit: vi.fn(), cancel: vi.fn(), resume: vi.fn(),
  uninstallIpc: vi.fn(), trackerDispose: vi.fn(),
}));
vi.mock('electron', () => ({
  app: state.app, BrowserWindow: class { constructor() { return state.window; } },
  dialog: state.dialog, net: { fetch: state.fetch }, Menu: { setApplicationMenu: vi.fn() },
  screen: { getAllDisplays: () => [], getPrimaryDisplay: () => ({ workArea: { x: 0, y: 0, width: 1220, height: 820 } }) },
  session: { defaultSession: { webRequest: { onHeadersReceived: vi.fn() }, setPermissionRequestHandler: vi.fn(), setPermissionCheckHandler: vi.fn() } },
}));
vi.mock('./cli', async importOriginal => ({ ...await importOriginal<typeof import('./cli')>(), productArguments: () => [] }));
vi.mock('./user-data-upgrade', () => ({ migrateUserData: async () => {} }));
vi.mock('./repository', () => ({ Repository: class { constructor() { return state.repository; } } }));
vi.mock('./service', () => ({ SessionService: class {} }));
vi.mock('./terminal/service', () => ({ EmbeddedSessionService: class {
  initialize = state.service.initialize;
  ownedTerminalCount = state.service.ownedTerminalCount;
  closeForUpdate = state.service.closeForUpdate;
  dispose = state.service.dispose;
} }));
vi.mock('./ipc', () => ({ installIpc: (_window: unknown, _url: unknown, _service: unknown, _ready: unknown, updates: SelfUpdater) => {
  state.updater = updates; return state.uninstallIpc;
} }));
vi.mock('./window-state', async importOriginal => ({
  ...await importOriginal<typeof import('./window-state')>(),
  trackWindowState: () => ({ dispose: state.trackerDispose }),
}));
vi.mock('./update/platform-installer', () => ({ createUpdatePlatform: async () => ({
  supported: true, reason: null, assetName: () => 'ShellfoxSetup.exe', prepare: state.prepare,
}) }));
const installer = Buffer.from('verified test installer');
const release = {
  tag_name: 'v0.2.6', assets: [{ name: 'ShellfoxSetup.exe', size: installer.length,
    browser_download_url: 'https://github.com/gee666/shellfox/releases/download/v0.2.6/ShellfoxSetup.exe',
    digest: 'sha256:' + createHash('sha256').update(installer).digest('hex') }],
};
function listener(mock: typeof state.app.on, event: string): (event: { preventDefault(): void }) => void {
  const call = mock.mock.calls.find(([name]) => name === event);
  if (!call) throw new Error(`Missing ${event} listener`);
  return call[1];
}
function quit() {
  const event = { preventDefault: vi.fn() };
  listener(state.app.on, 'before-quit')(event);
  return event;
}
async function stage() {
  await state.updater!.download();
  await expect.poll(async () => (await state.updater!.status()).phase).toBe('ready');
}
beforeEach(async () => {
  vi.resetModules(); vi.resetAllMocks(); vi.stubGlobal('__TEST_BUILD__', false);
  await mkdir('tmp', { recursive: true }); state.root = await mkdtemp(path.resolve('tmp/quit-update-test-'));
  vi.stubGlobal('process', { ...process, resourcesPath: state.root });
  state.userData = state.root; state.owned = 0; state.committed = false; state.updater = undefined;
  state.app.getPath.mockImplementation(name => name === 'userData' ? state.userData : state.root);
  state.app.setPath.mockImplementation((name, value) => { if (name === 'userData') state.userData = value; });
  state.app.quit.mockImplementation(() => { quit(); });
  state.dialog.showMessageBoxSync.mockReturnValue(1);
  state.service.ownedTerminalCount.mockImplementation(() => state.owned);
  state.service.closeForUpdate.mockImplementation(async () => { state.owned = 0; return state.resume; });
  state.service.dispose.mockResolvedValue(undefined);
  state.fetch.mockImplementation(async url => url.includes('/releases/latest') ? Response.json(release) : new Response(installer));
  state.prepare.mockResolvedValue(state.start);
  state.start.mockImplementation(async () => ({
    get committed() { return state.committed; }, commit: state.commit, cancel: state.cancel,
  }));
  state.commit.mockImplementation(async () => { state.committed = true; }); state.cancel.mockResolvedValue(undefined);
  await import('./index');
  await expect.poll(() => state.app.on.mock.calls.some(([name]) => name === 'before-quit')).toBe(true);
  expect(state.app.exit).not.toHaveBeenCalled(); expect(state.dialog.showErrorBox).not.toHaveBeenCalled();
});
afterEach(async () => {
  await state.updater?.dispose();
  vi.unstubAllGlobals(); await rm(state.root, { recursive: true, force: true });
});

describe('main automatic update on close/quit', () => {
  it.each(['close', 'quit'])('installs a ready update on %s before final cleanup, without an idle-app prompt', async action => {
    await stage();
    const event = { preventDefault: vi.fn() };
    if (action === 'close') listener(state.window.on, 'close')(event);
    else listener(state.app.on, 'before-quit')(event);
    expect(event.preventDefault).toHaveBeenCalledTimes(1);
    await expect.poll(() => state.repository.close.mock.calls.length).toBe(1);
    expect(state.dialog.showMessageBoxSync).not.toHaveBeenCalled();
    expect(state.start).toHaveBeenCalledTimes(1); expect(state.commit).toHaveBeenCalledTimes(1);
    expect(state.start.mock.invocationCallOrder[0]).toBeLessThan(state.service.closeForUpdate.mock.invocationCallOrder[0]!);
    expect(state.service.closeForUpdate.mock.invocationCallOrder[0]).toBeLessThan(state.commit.mock.invocationCallOrder[0]!);
    expect(state.commit.mock.invocationCallOrder[0]).toBeLessThan(state.service.dispose.mock.invocationCallOrder[0]!);
    expect(state.service.dispose).toHaveBeenCalledTimes(1); expect(state.cancel).not.toHaveBeenCalled();
    expect(state.uninstallIpc).toHaveBeenCalledTimes(1); expect(state.trackerDispose).toHaveBeenCalledTimes(1);
    expect(await readdir(state.root)).toContainEqual(expect.stringMatching(/^shellfox-update-/));
    expect(quit().preventDefault).not.toHaveBeenCalled(); expect(state.start).toHaveBeenCalledTimes(1);
  });
  it('confirms owned terminal shutdown once, and cancellation keeps the download and terminals for retry', async () => {
    await stage(); state.owned = 2; state.dialog.showMessageBoxSync.mockReturnValueOnce(0);
    quit();
    await expect.poll(async () => ({ phase: (await state.updater!.status()).phase, prompts: state.dialog.showMessageBoxSync.mock.calls.length })).toEqual({ phase: 'ready', prompts: 1 });
    expect(state.start).not.toHaveBeenCalled(); expect(state.service.dispose).not.toHaveBeenCalled();
    expect(state.service.closeForUpdate).not.toHaveBeenCalled(); expect(state.repository.close).not.toHaveBeenCalled();
    expect(state.owned).toBe(2); expect(await readdir(state.root)).toContainEqual(expect.stringMatching(/^shellfox-update-/));
    quit(); await expect.poll(() => state.repository.close.mock.calls.length).toBe(1);
    expect(state.dialog.showMessageBoxSync).toHaveBeenCalledTimes(2); expect(state.start).toHaveBeenCalledTimes(1);
  });
  it('normally quits an available but undownloaded update without launching an installer', async () => {
    await state.updater!.status(); quit();
    await expect.poll(() => state.repository.close.mock.calls.length).toBe(1);
    expect(state.prepare).not.toHaveBeenCalled(); expect(state.start).not.toHaveBeenCalled();
    expect(state.service.closeForUpdate).not.toHaveBeenCalled(); expect(state.service.dispose).toHaveBeenCalledTimes(1);
  });
  it('preserves explicit install confirmation, even when no terminals are owned', async () => {
    await stage();
    expect(await state.updater!.install()).toMatchObject({ ok: true, value: { phase: 'installing' } });
    expect(state.dialog.showMessageBoxSync).toHaveBeenCalledTimes(1);
    expect(state.repository.close).toHaveBeenCalledTimes(1); expect(state.commit).toHaveBeenCalledTimes(1);
  });
  it('normally quits a failed download without launching an installer', async () => {
    state.fetch.mockImplementationOnce(async () => new Response('bad checksum'));
    await state.updater!.download(); await expect.poll(async () => (await state.updater!.status()).phase).toBe('error');
    quit(); await expect.poll(() => state.repository.close.mock.calls.length).toBe(1);
    expect(state.start).not.toHaveBeenCalled(); expect(state.service.dispose).toHaveBeenCalledTimes(1);
  });
  it('keeps normal terminal quit confirmation when no update is ready', async () => {
    state.owned = 2; state.dialog.showMessageBoxSync.mockReturnValueOnce(0);
    quit(); await expect.poll(() => state.dialog.showMessageBoxSync.mock.calls.length).toBe(1);
    expect(state.service.dispose).not.toHaveBeenCalled(); expect(state.repository.close).not.toHaveBeenCalled();
    quit(); await expect.poll(() => state.repository.close.mock.calls.length).toBe(1);
    expect(state.dialog.showMessageBoxSync).toHaveBeenCalledTimes(2); expect(state.start).not.toHaveBeenCalled();
  });
  it('aborts an incomplete download on quit, without waiting for readiness or installing', async () => {
    let aborted = false;
    state.fetch.mockImplementationOnce(async (_url, init: RequestInit) => new Promise((_resolve, reject) => {
      init.signal!.addEventListener('abort', () => { aborted = true; reject(new Error('aborted')); });
    }));
    await state.updater!.download(); await expect.poll(() => state.fetch.mock.calls.length).toBe(2);
    quit(); await expect.poll(() => state.repository.close.mock.calls.length).toBe(1);
    expect(aborted).toBe(true); expect(state.prepare).not.toHaveBeenCalled(); expect(state.start).not.toHaveBeenCalled();
    expect(await readdir(state.root)).not.toContainEqual(expect.stringMatching(/^shellfox-update-/));
  });
  it.each(['startup', 'terminal close', 'commit'])('keeps the app open after %s failure and permits a later safe retry', async failure => {
    await stage();
    const effect = failure === 'startup' ? state.start : failure === 'terminal close' ? state.service.closeForUpdate : state.commit;
    effect.mockRejectedValueOnce(new Error('failed'));
    quit(); await expect.poll(() => state.dialog.showErrorBox.mock.calls.length).toBe(1);
    expect(state.service.dispose).not.toHaveBeenCalled(); expect(state.repository.close).not.toHaveBeenCalled();
    expect(state.committed).toBe(false);
    if (failure === 'startup') expect(state.service.closeForUpdate).not.toHaveBeenCalled();
    else expect(state.cancel).toHaveBeenCalledTimes(1);
    if (failure === 'commit') expect(state.resume).toHaveBeenCalledTimes(1);
    quit(); await expect.poll(() => state.repository.close.mock.calls.length).toBe(1);
    expect(state.committed).toBe(true); expect(state.service.dispose).toHaveBeenCalledTimes(1);
  });
  it('does not retry a committed installation when final cleanup fails', async () => {
    await stage(); state.service.dispose.mockRejectedValueOnce(new Error('cleanup failed'));
    const logged = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      quit(); await expect.poll(() => state.app.quit.mock.calls.length).toBe(1);
      expect(logged).toHaveBeenCalled(); expect(state.commit).toHaveBeenCalledTimes(1);
      expect(quit().preventDefault).not.toHaveBeenCalled();
      expect(state.start).toHaveBeenCalledTimes(1); expect(state.cancel).not.toHaveBeenCalled();
    } finally { logged.mockRestore(); }
  });
  it('ignores repeated close/quit and explicit install requests during handoff', async () => {
    await stage(); let ready!: () => void;
    state.service.closeForUpdate.mockImplementationOnce(() => new Promise(resolve => { ready = () => resolve(state.resume); }));
    quit(); await expect.poll(() => typeof ready).toBe('function');
    expect(quit().preventDefault).toHaveBeenCalled();
    listener(state.window.on, 'close')({ preventDefault: vi.fn() });
    expect(await state.updater!.install()).toMatchObject({ ok: false });
    expect(state.start).toHaveBeenCalledTimes(1); expect(state.commit).not.toHaveBeenCalled();
    ready(); await expect.poll(() => state.repository.close.mock.calls.length).toBe(1);
    expect(state.commit).toHaveBeenCalledTimes(1); expect(state.service.dispose).toHaveBeenCalledTimes(1);
  });
});
