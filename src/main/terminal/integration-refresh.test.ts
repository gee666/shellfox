import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { CliIntegrationDto, ExplorerIntegrationDto, Result } from '../../shared/contracts';
import { failure, success } from '../../shared/contracts';
import { PtyBackend } from './backend';
import { EmbeddedSessionService } from './service';
import { factoryFixture, MemoryRepository, profiles } from './test-fixtures';

vi.mock('./tracking', async original => ({ ...await original<typeof import('./tracking')>(), getProcessTrackingCapability: vi.fn(async () => ({ available: true, reason: null, helperPath: null })) }));
const originalPlatform = Object.getOwnPropertyDescriptor(process, 'platform');
const services: EmbeddedSessionService[] = []; let now = 0;
beforeEach(() => { Object.defineProperty(process, 'platform', { value: 'win32' }); now = 0; vi.spyOn(Date, 'now').mockImplementation(() => now); });
afterEach(async () => { await Promise.all(services.splice(0).map(service => service.dispose())); vi.restoreAllMocks(); if (originalPlatform) Object.defineProperty(process, 'platform', originalPlatform); });
const explorerState = (installed: boolean, reason: string | null = null): ExplorerIntegrationDto => ({ supported: true, installed, folderItemInstalled: installed, backgroundInstalled: installed, reason });
const cliState = (installed: boolean, reason: string | null = null): CliIntegrationDto => ({ supported: true, installed, command: 'shellfox', reason });
const flush = () => new Promise<void>(resolve => setImmediate(resolve));
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(done => { resolve = done; }); return { promise, resolve }; }
async function fixture(preference = false, initialize = true, installed = false) {
  let explorerActual = explorerState(installed), cliActual = cliState(installed);
  const repository = Object.assign(new MemoryRepository(), { cliPreference: () => preference, saveCliPreference: vi.fn() }); repository.preference = preference;
  const explorer = { get: vi.fn(async () => success(explorerActual)), set: vi.fn(async (installed: boolean) => { explorerActual = explorerState(installed); return success(explorerActual); }) };
  const cli = { binDir: '/fixture/bin', get: vi.fn(async () => success(cliActual)), set: vi.fn(async (installed: boolean) => { cliActual = cliState(installed); return success(cliActual); }) };
  const f = factoryFixture(), backend = new PtyBackend(f.options), service = new EmbeddedSessionService(repository, backend, () => ({ setWatch: vi.fn(), dispose: vi.fn() }), explorer, cli); services.push(service);
  if (initialize) await service.initialize();
  const state = (kind: 'explorer' | 'cli', installed: boolean) => { if (kind === 'explorer') explorerActual = explorerState(installed); else cliActual = cliState(installed); };
  return { service, backend, explorer, cli, repository, state };
}
it('repairs removed integrations without unnecessary notifications and emits one notification for changed failure metadata', async () => {
  const f = await fixture(), changed = vi.fn(); f.service.subscribe(changed);
  now = 5000; await f.service.refreshIntegrations(); expect(changed).not.toHaveBeenCalled();
  f.state('explorer', false); f.state('cli', false); now = 10000; await f.service.refreshIntegrations();
  expect(f.service.explorer.installed).toBe(true); expect(f.service.cli.installed).toBe(true); expect(changed).not.toHaveBeenCalled();
  f.state('explorer', false); f.state('cli', false); f.explorer.set.mockResolvedValueOnce(failure('AUTH_FAILED', 'Folder repair failed')); f.cli.set.mockResolvedValueOnce(failure('AUTH_FAILED', 'CLI repair failed'));
  now = 15000; await f.service.refreshIntegrations(); expect(changed).toHaveBeenCalledOnce(); expect(changed).toHaveBeenCalledWith(expect.objectContaining({ reason: 'settings' }));
  now = 19999; await f.service.refreshIntegrations(); expect(changed).toHaveBeenCalledOnce();
  now = 20000; await f.service.refreshIntegrations(); expect(f.service.cli.installed).toBe(true); expect(changed).toHaveBeenCalledTimes(2);
});
it('does not publish a read or begin a repair after disposal', async () => {
  const f = await fixture(), changed = vi.fn(), read = deferred<Result<ExplorerIntegrationDto>>(); f.service.subscribe(changed); f.explorer.get.mockReturnValueOnce(read.promise);
  now = 5000; const refreshing = f.service.refreshIntegrations(); await flush(); await f.service.dispose(); changed.mockClear();
  read.resolve(success(explorerState(false))); await refreshing; expect(f.service.explorer.installed).toBe(true); expect(changed).not.toHaveBeenCalled();
  expect(f.explorer.set).toHaveBeenCalledOnce(); expect(f.cli.get).toHaveBeenCalledOnce(); await f.service.refreshIntegrations(); expect(f.explorer.get).toHaveBeenCalledTimes(2);
});
it.each([false, true])('ignores historical preference %s and caches complete startup metadata for five seconds', async preference => {
  const f = await fixture(preference); await Promise.all([f.service.refreshIntegrations(), f.service.refreshIntegrations()]); now = 4999; await f.service.refreshIntegrations();
  expect(f.explorer.get).toHaveBeenCalledOnce(); expect(f.cli.get).toHaveBeenCalledOnce(); expect(f.explorer.set).toHaveBeenCalledExactlyOnceWith(true); expect(f.cli.set).toHaveBeenCalledExactlyOnceWith(true);
  now = 5000; await Promise.all([f.service.refreshIntegrations(), f.service.refreshIntegrations()]); expect(f.explorer.get).toHaveBeenCalledTimes(2); expect(f.cli.get).toHaveBeenCalledTimes(2);
  now = 9999; await f.service.refreshIntegrations(); expect(f.explorer.get).toHaveBeenCalledTimes(2);
  now = 10000; await f.service.refreshIntegrations(); expect(f.explorer.get).toHaveBeenCalledTimes(3); expect(f.cli.get).toHaveBeenCalledTimes(3);
});
it('starts freshness when all initialization finishes, not when early OS checks finish', async () => {
  const f = await fixture(false, false), discovery = deferred<typeof profiles>(); vi.spyOn(f.backend, 'refreshProfiles').mockImplementation(async () => { await discovery.promise; return profiles; });
  const initializing = f.service.initialize();
  try {
    await flush(); expect(f.explorer.get).toHaveBeenCalledOnce(); expect(f.cli.get).toHaveBeenCalledOnce();
    now = 10000; discovery.resolve(profiles); await initializing; now = 14999; await f.service.refreshIntegrations(); expect(f.explorer.get).toHaveBeenCalledOnce();
    now = 15000; await f.service.refreshIntegrations(); expect(f.explorer.get).toHaveBeenCalledTimes(2); expect(f.cli.get).toHaveBeenCalledTimes(2);
  } finally { discovery.resolve(profiles); await initializing; }
});
it('bounds failure metadata and retries after expiry without pretending installation succeeded', async () => {
  const f = await fixture(); f.explorer.get.mockResolvedValueOnce(failure('NATIVE_UNAVAILABLE', 'Registry unavailable')); f.cli.get.mockResolvedValueOnce(failure('NATIVE_UNAVAILABLE', 'PATH unavailable'));
  now = 5000; await f.service.refreshIntegrations(); expect(f.service.explorer).toMatchObject({ installed: false, reason: 'Registry unavailable' }); expect(f.service.cli).toMatchObject({ installed: false, reason: 'PATH unavailable' });
  now = 9999; await f.service.refreshIntegrations(); expect(f.explorer.get).toHaveBeenCalledTimes(2); expect(f.cli.get).toHaveBeenCalledTimes(2);
  now = 10000; await f.service.refreshIntegrations(); expect(f.service.explorer).toEqual(explorerState(true)); expect(f.service.cli).toEqual(cliState(true));
});
it('caches startup failure results without repeating subprocesses immediately', async () => {
  const f = await fixture(false, false); f.explorer.get.mockResolvedValue(failure('AUTH_FAILED', 'Foreign verb')); f.cli.get.mockResolvedValue(failure('NATIVE_UNAVAILABLE', 'No PATH'));
  await f.service.initialize(); await f.service.refreshIntegrations(); expect(f.explorer.get).toHaveBeenCalledOnce(); expect(f.cli.get).toHaveBeenCalledOnce(); expect(f.explorer.set).not.toHaveBeenCalled();
  now = 5000; await f.service.refreshIntegrations(); expect(f.explorer.get).toHaveBeenCalledTimes(2); expect(f.cli.get).toHaveBeenCalledTimes(2);
});
it('does not cache unexpected rejections and retries only the unfinished check', async () => {
  const f = await fixture(); now = 5000; f.cli.get.mockRejectedValueOnce(new Error('Unexpected read failure'));
  await expect(f.service.refreshIntegrations()).rejects.toThrow('Unexpected read failure'); expect(f.service.cli.installed).toBe(false);
  await f.service.refreshIntegrations(); expect(f.explorer.get).toHaveBeenCalledTimes(2); expect(f.cli.get).toHaveBeenCalledTimes(3);
});
it('rechecks metadata when the wall clock moves backwards', async () => { const f = await fixture(); now = -1; await f.service.refreshIntegrations(); expect(f.explorer.get).toHaveBeenCalledTimes(2); expect(f.cli.get).toHaveBeenCalledTimes(2); });
it('idempotent explicit ensures preserve independent freshness and never rewrite a healthy provider', async () => {
  const f = await fixture(false, true, true); now = 4000; expect(await f.service.setExplorerIntegration({ installed: true })).toMatchObject({ ok: true });
  now = 5000; await f.service.refreshIntegrations(); expect(f.explorer.get).toHaveBeenCalledTimes(2); expect(f.cli.get).toHaveBeenCalledTimes(2);
  await f.service.setCliIntegration({ installed: true }); now = 8999; await f.service.refreshIntegrations(); expect(f.explorer.get).toHaveBeenCalledTimes(2); expect(f.cli.get).toHaveBeenCalledTimes(3);
  now = 9000; await f.service.refreshIntegrations(); expect(f.explorer.get).toHaveBeenCalledTimes(3); expect(f.explorer.set).not.toHaveBeenCalled(); expect(f.cli.set).not.toHaveBeenCalled();
});
it.each(['result', 'throw'] as const)('keeps failed explicit repair metadata honest for %s', async mode => {
  const f = await fixture(); f.state('cli', false);
  if (mode === 'result') f.cli.set.mockResolvedValueOnce(failure('AUTH_FAILED', 'Foreign shim')); else f.cli.set.mockRejectedValueOnce(new Error('Partial write'));
  expect(await f.service.setCliIntegration({ installed: true })).toMatchObject({ ok: false }); expect(f.service.cli.installed).toBe(false);
  now = 5000; await f.service.refreshIntegrations(); expect(f.service.cli.installed).toBe(true); expect(f.repository.saveCliPreference).not.toHaveBeenCalled();
});
it('never writes historical preferences, even when their old storage methods would fail', async () => {
  const f = await fixture(), saveExplorer = vi.spyOn(f.repository, 'saveExplorerPreference').mockImplementation(() => { throw new Error('Old preference unavailable'); }); f.repository.saveCliPreference.mockImplementation(() => { throw new Error('Old preference unavailable'); });
  f.state('cli', false); f.state('explorer', false); expect(await f.service.setCliIntegration({ installed: true })).toMatchObject({ ok: true }); expect(await f.service.setExplorerIntegration({ installed: true })).toMatchObject({ ok: true });
  expect(saveExplorer).not.toHaveBeenCalled(); expect(f.repository.saveCliPreference).not.toHaveBeenCalled();
});
it.each([['explorer', false], ['cli', false], ['explorer', true], ['cli', true]] as const)('discards a stale %s read after a successful ensure, failure %s', async (kind, failedRead) => {
  const f = await fixture(), explorerRead = deferred<Result<ExplorerIntegrationDto>>(), cliRead = deferred<Result<CliIntegrationDto>>();
  if (kind === 'explorer') f.explorer.get.mockReturnValueOnce(explorerRead.promise); else f.cli.get.mockReturnValueOnce(cliRead.promise);
  now = 5000; const refreshing = f.service.refreshIntegrations();
  try {
    await flush(); const enabled = kind === 'explorer' ? await f.service.setExplorerIntegration({ installed: true }) : await f.service.setCliIntegration({ installed: true }); expect(enabled.ok).toBe(true);
    explorerRead.resolve(failedRead ? failure('NATIVE_UNAVAILABLE', 'Stale failure') : success(explorerState(false))); cliRead.resolve(failedRead ? failure('NATIVE_UNAVAILABLE', 'Stale failure') : success(cliState(false))); await refreshing;
    expect(f.service[kind].installed).toBe(true); expect(f.service[kind].reason).toBeNull(); await f.service.refreshIntegrations(); expect(f[kind].get).toHaveBeenCalledTimes(3);
  } finally { explorerRead.resolve(success(explorerState(false))); cliRead.resolve(success(cliState(false))); await refreshing; }
});
it.each(['explorer', 'cli'] as const)('never inspects partial %s writes or applies a read completed during repair', async kind => {
  const f = await fixture(false, true, true), explorerRead = deferred<Result<ExplorerIntegrationDto>>(), cliRead = deferred<Result<CliIntegrationDto>>(), explorerWrite = deferred<Result<ExplorerIntegrationDto>>(), cliWrite = deferred<Result<CliIntegrationDto>>();
  if (kind === 'explorer') { f.explorer.get.mockReturnValueOnce(explorerRead.promise); f.explorer.set.mockReturnValueOnce(explorerWrite.promise); } else { f.cli.get.mockReturnValueOnce(cliRead.promise); f.cli.set.mockReturnValueOnce(cliWrite.promise); }
  now = 5000; const refreshing = f.service.refreshIntegrations(); await flush(); f.state(kind, false);
  const setting = kind === 'explorer' ? f.service.setExplorerIntegration({ installed: true }) : f.service.setCliIntegration({ installed: true });
  try {
    await flush(); explorerRead.resolve(success(explorerState(false))); cliRead.resolve(success(cliState(false))); await refreshing; expect(f.service[kind].installed).toBe(true);
    await f.service.refreshIntegrations(); expect(f[kind].get).toHaveBeenCalledTimes(3);
    f.state(kind, true); explorerWrite.resolve(success(explorerState(true))); cliWrite.resolve(success(cliState(true))); await setting;
    expect(f.service[kind].installed).toBe(true); await f.service.refreshIntegrations(); expect(f[kind].get).toHaveBeenCalledTimes(3);
  } finally { explorerRead.resolve(success(explorerState(false))); cliRead.resolve(success(cliState(false))); explorerWrite.resolve(success(explorerState(true))); cliWrite.resolve(success(cliState(true))); await Promise.all([refreshing, setting]); }
});
it('rejects false directly before any provider read, write or preference mutation', async () => {
  const f = await fixture(); f.explorer.get.mockClear(); f.cli.get.mockClear(); f.explorer.set.mockClear(); f.cli.set.mockClear();
  expect(await f.service.setExplorerIntegration({ installed: false })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } }); expect(await f.service.setCliIntegration({ installed: false })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(f.explorer.get).not.toHaveBeenCalled(); expect(f.cli.get).not.toHaveBeenCalled(); expect(f.explorer.set).not.toHaveBeenCalled(); expect(f.cli.set).not.toHaveBeenCalled(); expect(f.service.cli.installed).toBe(true);
});
it('invalidates dependent Linux metadata and repairs it without allowing disable operations', async () => {
  Object.defineProperty(process, 'platform', { value: 'linux' }); const f = await fixture(false, true, true); f.state('cli', false);
  await f.service.setExplorerIntegration({ installed: true }); await f.service.refreshIntegrations(); expect(f.service.cli.installed).toBe(true); expect(f.cli.set).toHaveBeenCalledExactlyOnceWith(true);
  await f.service.setCliIntegration({ installed: true }); await f.service.refreshIntegrations(); expect(f.service.explorer.installed).toBe(true); expect(f.service.cli.installed).toBe(true); expect(f.explorer.set).not.toHaveBeenCalled();
});
it('discards a Linux menu read invalidated by a CLI repair', async () => {
  Object.defineProperty(process, 'platform', { value: 'linux' }); const f = await fixture(false, true, true), read = deferred<Result<ExplorerIntegrationDto>>(); f.explorer.get.mockReturnValueOnce(read.promise);
  now = 5000; const refreshing = f.service.refreshIntegrations();
  try {
    await flush(); f.state('cli', false); await f.service.setCliIntegration({ installed: true }); read.resolve(success(explorerState(false, 'Stale menu read'))); await refreshing;
    expect(f.service.explorer.reason).toBeNull(); expect(f.service.cli.installed).toBe(true); await f.service.refreshIntegrations(); expect(f.explorer.get).toHaveBeenCalledTimes(3); expect(f.service.explorer.installed).toBe(true);
  } finally { read.resolve(success(explorerState(true))); await refreshing; }
});
