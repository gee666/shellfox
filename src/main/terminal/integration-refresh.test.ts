import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { CliIntegrationDto, ExplorerIntegrationDto, Result } from '../../shared/contracts';
import { failure, success } from '../../shared/contracts';
import { PtyBackend } from './backend';
import { EmbeddedSessionService } from './service';
import { factoryFixture, MemoryRepository, profiles } from './test-fixtures';

vi.mock('./tracking', async importOriginal => ({
  ...await importOriginal<typeof import('./tracking')>(),
  getProcessTrackingCapability: vi.fn(async () => ({ available: true, reason: null, helperPath: null })),
}));
const originalPlatform = Object.getOwnPropertyDescriptor(process, 'platform')!;
const services: EmbeddedSessionService[] = [];
let now = 0;
beforeEach(() => {
  Object.defineProperty(process, 'platform', { value: 'win32' });
  now = 0; vi.spyOn(Date, 'now').mockImplementation(() => now);
});
afterEach(async () => {
  await Promise.all(services.splice(0).map(service => service.dispose()));
  vi.restoreAllMocks(); Object.defineProperty(process, 'platform', originalPlatform);
});
const explorerState = (installed: boolean, reason: string | null = null): ExplorerIntegrationDto => ({ supported: true, installed, folderItemInstalled: installed, backgroundInstalled: installed, reason });
const cliState = (installed: boolean, reason: string | null = null): CliIntegrationDto => ({ supported: true, installed, command: 'shellfox start <path>', reason });
const flush = () => new Promise<void>(resolve => setImmediate(resolve));
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
}
async function fixture(enabled = false, initialize = true) {
  const repository = Object.assign(new MemoryRepository(), { cliPreference: () => enabled, saveCliPreference: vi.fn() });
  repository.preference = enabled;
  const explorer = { get: vi.fn(async () => success(explorerState(enabled))), set: vi.fn(async (installed: boolean) => success(explorerState(installed))) };
  const cli = { binDir: '/fixture/bin', get: vi.fn(async () => success(cliState(enabled))), set: vi.fn(async (installed: boolean) => success(cliState(installed))) };
  const f = factoryFixture(), backend = new PtyBackend(f.options);
  const service = new EmbeddedSessionService(repository, backend, () => ({ setWatch: vi.fn(), dispose: vi.fn() }), explorer, cli);
  services.push(service);
  if (initialize) await service.initialize();
  return { service, backend, explorer, cli, repository };
}

it.each([false, true])('reuses startup metadata for five seconds, saved opt-in %s', async enabled => {
  const f = await fixture(enabled);
  await Promise.all([f.service.refreshIntegrations(), f.service.refreshIntegrations()]);
  now = 4999; await f.service.refreshIntegrations();
  expect(f.explorer.get).toHaveBeenCalledTimes(enabled ? 0 : 1);
  expect(f.cli.get).toHaveBeenCalledTimes(enabled ? 0 : 1);
  expect(f.explorer.set).toHaveBeenCalledTimes(enabled ? 1 : 0);
  expect(f.cli.set).toHaveBeenCalledTimes(enabled ? 1 : 0);
  now = 5000;
  await Promise.all([f.service.refreshIntegrations(), f.service.refreshIntegrations()]);
  expect(f.explorer.get).toHaveBeenCalledTimes(enabled ? 1 : 2);
  expect(f.cli.get).toHaveBeenCalledTimes(enabled ? 1 : 2);
  now = 9999; await f.service.refreshIntegrations();
  expect(f.explorer.get).toHaveBeenCalledTimes(enabled ? 1 : 2);
  now = 10000; await f.service.refreshIntegrations();
  expect(f.explorer.get).toHaveBeenCalledTimes(enabled ? 2 : 3);
  expect(f.cli.get).toHaveBeenCalledTimes(enabled ? 2 : 3);
});

it('starts startup freshness when initialization finishes, not when early integration checks finish', async () => {
  const f = await fixture(false, false), discovery = deferred<typeof profiles>();
  vi.spyOn(f.backend, 'refreshProfiles').mockImplementation(async () => { await discovery.promise; return profiles; });
  const initializing = f.service.initialize();
  try {
    await flush(); expect(f.explorer.get).toHaveBeenCalledOnce(); expect(f.cli.get).toHaveBeenCalledOnce();
    now = 10000; discovery.resolve(profiles); await initializing;
    now = 14999; await f.service.refreshIntegrations();
    expect(f.explorer.get).toHaveBeenCalledOnce(); expect(f.cli.get).toHaveBeenCalledOnce();
    now = 15000; await f.service.refreshIntegrations();
    expect(f.explorer.get).toHaveBeenCalledTimes(2); expect(f.cli.get).toHaveBeenCalledTimes(2);
  } finally { discovery.resolve(profiles); await initializing; }
});

it('bounds returned failure metadata too, then retries after expiry', async () => {
  const f = await fixture(true);
  f.explorer.get.mockResolvedValueOnce(failure('NATIVE_UNAVAILABLE', 'Registry unavailable'));
  f.cli.get.mockResolvedValueOnce(failure('NATIVE_UNAVAILABLE', 'PATH unavailable'));
  now = 5000; await f.service.refreshIntegrations();
  expect(f.service.explorer).toMatchObject({ installed: false, reason: 'Registry unavailable' });
  expect(f.service.cli).toMatchObject({ installed: false, reason: 'PATH unavailable' });
  now = 9999; await f.service.refreshIntegrations();
  expect(f.explorer.get).toHaveBeenCalledOnce(); expect(f.cli.get).toHaveBeenCalledOnce();
  now = 10000; await f.service.refreshIntegrations();
  expect(f.service.explorer).toEqual(explorerState(true)); expect(f.service.cli).toEqual(cliState(true));
});

it('marks initialization failure metadata fresh instead of immediately repeating failing subprocesses', async () => {
  const f = await fixture(false, false);
  f.explorer.get.mockResolvedValue(failure('AUTH_FAILED', 'Foreign verb'));
  f.cli.get.mockResolvedValue(failure('NATIVE_UNAVAILABLE', 'No PATH'));
  await f.service.initialize(); await f.service.refreshIntegrations();
  expect(f.explorer.get).toHaveBeenCalledOnce(); expect(f.cli.get).toHaveBeenCalledOnce();
  now = 5000; await f.service.refreshIntegrations();
  expect(f.explorer.get).toHaveBeenCalledTimes(2); expect(f.cli.get).toHaveBeenCalledTimes(2);
});

it('does not cache unexpected rejections and retries only the unfinished check', async () => {
  const f = await fixture(); now = 5000;
  f.cli.get.mockRejectedValueOnce(new Error('Unexpected read failure'));
  await expect(f.service.refreshIntegrations()).rejects.toThrow('Unexpected read failure');
  await f.service.refreshIntegrations();
  expect(f.explorer.get).toHaveBeenCalledTimes(2); expect(f.cli.get).toHaveBeenCalledTimes(3);
});

it('rechecks metadata if the wall clock moves backwards', async () => {
  const f = await fixture(); now = -1; await f.service.refreshIntegrations();
  expect(f.explorer.get).toHaveBeenCalledTimes(2); expect(f.cli.get).toHaveBeenCalledTimes(2);
});

it('successful explicit setters preserve fresh states without extending the other integration freshness', async () => {
  const f = await fixture(); now = 4000;
  expect(await f.service.setExplorerIntegration({ installed: true })).toMatchObject({ ok: true });
  now = 5000; await f.service.refreshIntegrations();
  expect(f.service.explorer).toEqual(explorerState(true)); expect(f.explorer.get).toHaveBeenCalledOnce();
  expect(f.cli.get).toHaveBeenCalledTimes(2);
  expect(await f.service.setCliIntegration({ installed: true })).toMatchObject({ ok: true });
  now = 8999; await f.service.refreshIntegrations();
  expect(f.service.cli).toEqual(cliState(true)); expect(f.cli.get).toHaveBeenCalledTimes(2);
  expect(f.explorer.get).toHaveBeenCalledOnce();
  now = 9000; await f.service.refreshIntegrations();
  expect(f.explorer.get).toHaveBeenCalledTimes(2); expect(f.cli.get).toHaveBeenCalledTimes(2);
  expect(f.explorer.set).toHaveBeenCalledWith(true); expect(f.cli.set).toHaveBeenCalledWith(true);
});

it.each(['result', 'throw', 'storage'] as const)('invalidates freshness when an explicit mutation fails via %s', async mode => {
  const f = await fixture();
  if (mode === 'result') f.cli.set.mockResolvedValueOnce(failure('AUTH_FAILED', 'Foreign shim'));
  else if (mode === 'throw') f.cli.set.mockRejectedValueOnce(new Error('Partial write'));
  else vi.spyOn(f.repository, 'saveCliPreference').mockImplementationOnce(() => { throw new Error('Storage unavailable'); });
  expect(await f.service.setCliIntegration({ installed: true })).toMatchObject({ ok: false });
  await f.service.refreshIntegrations();
  expect(f.cli.get).toHaveBeenCalledTimes(2); expect(f.explorer.get).toHaveBeenCalledOnce();
  expect(f.service.cli).toEqual(cliState(false));
});

it.each([
  ['explorer', false], ['cli', false], ['explorer', true], ['cli', true],
] as const)('discards stale %s refresh results after a successful mutation, failed read %s', async (kind, failedRead) => {
  const f = await fixture();
  const explorerRead = deferred<Result<ExplorerIntegrationDto>>(), cliRead = deferred<Result<CliIntegrationDto>>();
  if (kind === 'explorer') f.explorer.get.mockReturnValueOnce(explorerRead.promise);
  else f.cli.get.mockReturnValueOnce(cliRead.promise);
  now = 5000; const refreshing = f.service.refreshIntegrations();
  try {
    await flush(); expect(f[kind].get).toHaveBeenCalledTimes(2);
    const changed = kind === 'explorer' ? await f.service.setExplorerIntegration({ installed: true }) : await f.service.setCliIntegration({ installed: true });
    expect(changed).toMatchObject({ ok: true });
    explorerRead.resolve(failedRead ? failure('NATIVE_UNAVAILABLE', 'Stale failure') : success(explorerState(false)));
    cliRead.resolve(failedRead ? failure('NATIVE_UNAVAILABLE', 'Stale failure') : success(cliState(false)));
    await refreshing;
    expect(f.service[kind].installed).toBe(true); expect(f.service[kind].reason).toBeNull();
    await f.service.refreshIntegrations(); expect(f[kind].get).toHaveBeenCalledTimes(2);
  } finally {
    explorerRead.resolve(success(explorerState(false))); cliRead.resolve(success(cliState(false))); await refreshing;
  }
});

it.each(['explorer', 'cli'] as const)('does not apply a refresh completed during a pending %s mutation or inspect partial writes', async kind => {
  const f = await fixture(true);
  const explorerRead = deferred<Result<ExplorerIntegrationDto>>(), cliRead = deferred<Result<CliIntegrationDto>>();
  const explorerWrite = deferred<Result<ExplorerIntegrationDto>>(), cliWrite = deferred<Result<CliIntegrationDto>>();
  if (kind === 'explorer') { f.explorer.get.mockReturnValueOnce(explorerRead.promise); f.explorer.set.mockReturnValueOnce(explorerWrite.promise); }
  else { f.cli.get.mockReturnValueOnce(cliRead.promise); f.cli.set.mockReturnValueOnce(cliWrite.promise); }
  now = 5000; const refreshing = f.service.refreshIntegrations(); await flush();
  const setting = kind === 'explorer' ? f.service.setExplorerIntegration({ installed: false }) : f.service.setCliIntegration({ installed: false });
  try {
    await flush();
    explorerRead.resolve(success(explorerState(false))); cliRead.resolve(success(cliState(false))); await refreshing;
    expect(f.service[kind].installed).toBe(true);
    await f.service.refreshIntegrations(); expect(f[kind].get).toHaveBeenCalledOnce();
    explorerWrite.resolve(success(explorerState(false))); cliWrite.resolve(success(cliState(false))); await setting;
    expect(f.service[kind].installed).toBe(false);
    await f.service.refreshIntegrations(); expect(f[kind].get).toHaveBeenCalledOnce();
  } finally {
    explorerRead.resolve(success(explorerState(false))); cliRead.resolve(success(cliState(false)));
    explorerWrite.resolve(success(explorerState(false))); cliWrite.resolve(success(cliState(false)));
    await Promise.all([refreshing, setting]);
  }
});

it('invalidates dependent Linux metadata without overwriting fresh explicit setter results', async () => {
  Object.defineProperty(process, 'platform', { value: 'linux' });
  const f = await fixture();
  f.cli.get.mockResolvedValue(success(cliState(true)));
  await f.service.setExplorerIntegration({ installed: true }); await f.service.refreshIntegrations();
  expect(f.explorer.get).toHaveBeenCalledOnce(); expect(f.cli.get).toHaveBeenCalledTimes(2);
  expect(f.service.explorer.installed).toBe(true); expect(f.service.cli.installed).toBe(true);
  await f.service.setCliIntegration({ installed: false }); await f.service.refreshIntegrations();
  expect(f.explorer.get).toHaveBeenCalledTimes(2); expect(f.cli.get).toHaveBeenCalledTimes(2);
  expect(f.service.explorer.installed).toBe(false); expect(f.service.cli.installed).toBe(false);
});

it('discards a Linux menu read invalidated by a CLI mutation', async () => {
  Object.defineProperty(process, 'platform', { value: 'linux' });
  const f = await fixture(true), read = deferred<Result<ExplorerIntegrationDto>>();
  f.explorer.get.mockReturnValueOnce(read.promise).mockResolvedValue(success(explorerState(false)));
  now = 5000; const refreshing = f.service.refreshIntegrations();
  try {
    await flush(); await f.service.setCliIntegration({ installed: false });
    read.resolve(success(explorerState(true, 'Stale menu read'))); await refreshing;
    expect(f.service.explorer.reason).toBeNull(); expect(f.service.cli.installed).toBe(false);
    await f.service.refreshIntegrations();
    expect(f.explorer.get).toHaveBeenCalledTimes(2); expect(f.service.explorer.installed).toBe(false);
    expect(f.cli.get).not.toHaveBeenCalled();
  } finally { read.resolve(success(explorerState(true))); await refreshing; }
});
