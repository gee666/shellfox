import { EventEmitter } from 'node:events';
import path from 'node:path';
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { NativeBackend } from '../shared/native-port';
import { failure, success, type CliIntegrationDto, type ExplorerIntegrationDto } from '../shared/contracts';
import { unavailableProbe } from './defaults';
const mocks = vi.hoisted(() => ({ ready: vi.fn(async () => {}), spawn: vi.fn(), repository: vi.fn(() => { throw new Error('Installer must not open the host database'); }) }));
vi.mock('electron', () => ({ app: { whenReady: mocks.ready } }));
vi.mock('node:child_process', () => ({ spawn: mocks.spawn }));
vi.mock('./repository', () => ({ Repository: mocks.repository }));
import { handleEmbeddedInstaller, handleInstaller } from './installer';

const directories: string[] = [];
beforeEach(() => {
  vi.clearAllMocks(); mocks.spawn.mockImplementation(() => { const child = Object.assign(new EventEmitter(), { kill: vi.fn() }); queueMicrotask(() => child.emit('exit', 0)); return child; });
});
afterEach(async () => { for (const directory of directories.splice(0)) await rm(directory, { recursive: true, force: true }); });
const folder = (installed = false, supported = true): ExplorerIntegrationDto => ({ supported, installed, folderItemInstalled: installed, backgroundInstalled: installed, reason: supported ? null : 'Unsupported file manager' });
const command = (installed = false, supported = true): CliIntegrationDto => ({ supported, installed, command: 'shellfox', reason: supported ? null : 'Unsupported CLI' });
function ports(supported = true) {
  return { explorer: { get: vi.fn(async () => success(folder(false, supported))), set: vi.fn(async (installed: boolean) => success(folder(installed, supported))) }, cli: { binDir: '/fixture/bin', get: vi.fn(async () => success(command(false, supported))), set: vi.fn(async (installed: boolean) => success(command(installed, supported))) } };
}
async function nativeFixture() {
  await mkdir('tmp', { recursive: true }); const root = await mkdtemp(path.resolve('tmp/installer-test-')); directories.push(root);
  const unsubscribe = vi.fn();
  const backend = { initialize: vi.fn(async () => success(unavailableProbe('Fixture'))), launch: vi.fn(async () => failure('UNSUPPORTED', 'unused')), focus: vi.fn(async () => success({ focused: true as const })), verifyTarget: vi.fn<NativeBackend['verifyTarget']>(async input => success(input)), setWatch: vi.fn(async () => success({ configured: true as const })), getExplorerIntegration: vi.fn(async () => success(folder())), setExplorerIntegration: vi.fn<NativeBackend['setExplorerIntegration']>(async input => success(folder(input.installed))), subscribe: vi.fn(() => unsubscribe), dispose: vi.fn(async () => {}) } satisfies NativeBackend;
  return { backend, unsubscribe, init: { userDataDir: root, helperDir: root, shellScriptDir: root, packagedExecutable: process.execPath } };
}
it.each(['--squirrel-install', '--squirrel-updated'])('always installs supported embedded providers for %s without consulting historical database preferences', async event => {
  const { explorer, cli } = ports(); await handleEmbeddedInstaller(event, explorer, cli);
  expect(explorer.set).toHaveBeenCalledExactlyOnceWith(true); expect(cli.set).toHaveBeenCalledExactlyOnceWith(true); expect(mocks.repository).not.toHaveBeenCalled();
  expect(mocks.spawn).toHaveBeenCalledWith(path.resolve(path.dirname(process.execPath), '..', 'Update.exe'), ['--createShortcut', path.basename(process.execPath)], expect.objectContaining({ shell: false }));
});
it('retains internal owned-provider removal on actual application uninstall', async () => {
  const { explorer, cli } = ports(); await handleEmbeddedInstaller('--squirrel-uninstall', explorer, cli);
  expect(explorer.set).toHaveBeenCalledExactlyOnceWith(false); expect(cli.set).toHaveBeenCalledExactlyOnceWith(false); expect(explorer.get).not.toHaveBeenCalled(); expect(cli.get).not.toHaveBeenCalled(); expect(mocks.repository).not.toHaveBeenCalled();
  expect(mocks.spawn).toHaveBeenCalledWith(expect.any(String), ['--removeShortcut', path.basename(process.execPath)], expect.any(Object));
});
it('skips unsupported installations and never changes a read-only headless CLI', async () => {
  const { explorer, cli } = ports(false); const readonly = { ...cli, readOnly: true as const };
  await handleEmbeddedInstaller('--squirrel-install', explorer, readonly); expect(explorer.set).not.toHaveBeenCalled(); expect(cli.set).not.toHaveBeenCalled(); expect(cli.get).not.toHaveBeenCalled();
  await handleEmbeddedInstaller('--squirrel-install', explorer, cli); expect(cli.get).toHaveBeenCalledOnce(); expect(cli.set).not.toHaveBeenCalled();
});
it('preserves provider ownership failures rather than overwriting foreign integrations', async () => {
  const { explorer, cli } = ports(); explorer.set.mockResolvedValueOnce(failure('AUTH_FAILED', 'Foreign owner'));
  await expect(handleEmbeddedInstaller('--squirrel-updated', explorer, cli)).rejects.toThrow('Installer integration failed'); expect(cli.set).not.toHaveBeenCalled(); expect(mocks.spawn).not.toHaveBeenCalled();
});
it.each([['--squirrel-install', true], ['--squirrel-uninstall', false]] as const)('uses the native ownership-checked provider for %s', async (event, installed) => {
  const f = await nativeFixture(); await handleInstaller(event, f.backend, f.init);
  expect(f.backend.setExplorerIntegration).toHaveBeenCalledExactlyOnceWith({ installed, executablePath: process.execPath }); expect(f.unsubscribe).toHaveBeenCalledOnce(); expect(f.backend.dispose).toHaveBeenCalledOnce(); expect(mocks.repository).not.toHaveBeenCalled();
});
it('does not fake native installation on an unsupported platform and still disposes its backend', async () => {
  const f = await nativeFixture(); f.backend.getExplorerIntegration.mockResolvedValue(success(folder(false, false))); await handleInstaller('--squirrel-install', f.backend, f.init);
  expect(f.backend.setExplorerIntegration).not.toHaveBeenCalled(); expect(f.backend.dispose).toHaveBeenCalledOnce();
});
it('does nothing for obsolete installer events', async () => {
  const { explorer, cli } = ports(); await handleEmbeddedInstaller('--squirrel-obsolete', explorer, cli); expect(mocks.ready).not.toHaveBeenCalled(); expect(explorer.get).not.toHaveBeenCalled(); expect(mocks.spawn).not.toHaveBeenCalled();
});
