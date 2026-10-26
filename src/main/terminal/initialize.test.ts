import { afterEach, expect, it, vi } from 'vitest';
import { failure, success } from '../../shared/contracts';
import type { CliIntegrationDto, ExplorerIntegrationDto, Result, TerminalProfilesDto } from '../../shared/contracts';
import { PtyBackend } from './backend';
import { EmbeddedSessionService } from './service';
import { factoryFixture, MemoryRepository, profiles } from './test-fixtures';
import { getProcessTrackingCapability, type ProcessTrackingCapability } from './tracking';

vi.mock('./tracking', async original => ({ ...await original<typeof import('./tracking')>(), getProcessTrackingCapability: vi.fn() }));
const platformDescriptor = Object.getOwnPropertyDescriptor(process, 'platform');
afterEach(() => { if (platformDescriptor) Object.defineProperty(process, 'platform', platformDescriptor); vi.restoreAllMocks(); vi.resetAllMocks(); });
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(done => { resolve = done; }); return { promise, resolve }; }
const flush = () => new Promise<void>(resolve => setImmediate(resolve));
const explorerState = (installed: boolean): ExplorerIntegrationDto => ({ supported: true, installed, folderItemInstalled: installed, backgroundInstalled: installed, reason: null });
const cliState = (installed: boolean): CliIntegrationDto => ({ supported: true, installed, command: 'shellfox', reason: null });

it.each([
  ['win32', true, 'backend'], ['win32', false, 'tracking'], ['linux', true, 'backend'], ['linux', false, 'tracking'],
] as const)('overlaps independent %s readiness, ignores old preference %s, %s finishes first', async (platform, enabled, first) => {
  Object.defineProperty(process, 'platform', { value: platform });
  const repository = new MemoryRepository(); repository.preference = enabled; repository.config.adapterId = 'windows-terminal'; repository.config.pythonPath = '/saved/python';
  const saveCli = vi.fn(); Object.assign(repository, { cliPreference: () => enabled, saveCliPreference: saveCli });
  const discovery = deferred<TerminalProfilesDto>(), explorerDone = deferred<Result<ExplorerIntegrationDto>>(), cliDone = deferred<Result<CliIntegrationDto>>(), trackingDone = deferred<ProcessTrackingCapability>();
  const explorer = { get: vi.fn(() => explorerDone.promise), set: vi.fn(async () => success(explorerState(true))) };
  const cli = { binDir: '/fixture/bin', get: vi.fn(() => cliDone.promise), set: vi.fn(async () => success(cliState(true))) };
  const f = factoryFixture(), discover = vi.fn(() => discovery.promise), backend = new PtyBackend({ ...f.options, discover });
  const python = vi.spyOn(backend, 'configurePythonPath'), getProfiles = vi.spyOn(backend, 'getProfiles'), transaction = vi.spyOn(repository, 'transaction'), saveExplorer = vi.spyOn(repository, 'saveExplorerPreference'), watch = vi.fn(), changed = vi.fn();
  vi.mocked(getProcessTrackingCapability).mockReturnValue(trackingDone.promise);
  const service = new EmbeddedSessionService(repository, backend, () => ({ setWatch: watch, dispose: vi.fn() }), explorer, cli); service.subscribe(changed);
  let ready = false; const initializing = service.initialize().then(() => { ready = true; });
  const tracking = { available: false, reason: 'Tracking preflight failed', helperPath: null };
  try {
    await flush(); expect(python).toHaveBeenCalledWith('/saved/python'); expect(discover).toHaveBeenCalledOnce(); expect(getProcessTrackingCapability).toHaveBeenCalledOnce();
    expect(explorer.get).toHaveBeenCalledOnce(); expect(cli.get).toHaveBeenCalledTimes(platform === 'linux' ? 0 : 1); expect(explorer.set).not.toHaveBeenCalled(); expect(getProfiles).not.toHaveBeenCalled();
    if (first === 'backend') discovery.resolve(profiles); else trackingDone.resolve(tracking);
    await flush(); expect(ready).toBe(false); expect(transaction).not.toHaveBeenCalled(); expect(watch).not.toHaveBeenCalled();
    explorerDone.resolve(success(explorerState(false))); await flush(); expect(explorer.set).toHaveBeenCalledExactlyOnceWith(true); expect(cli.get).toHaveBeenCalledOnce();
    cliDone.resolve(success(cliState(false))); await flush(); expect(cli.set).toHaveBeenCalledExactlyOnceWith(true); expect(changed).not.toHaveBeenCalled(); expect(ready).toBe(false);
    if (first === 'backend') trackingDone.resolve(tracking); else discovery.resolve(profiles);
    await initializing; expect(ready).toBe(true); expect(transaction).toHaveBeenCalledOnce(); expect(repository.settings().adapterId).toBe('embedded-pty');
    expect(watch).toHaveBeenCalledWith([], repository.settings().processRules); expect(service.probe).toMatchObject({ available: true, capabilities: { processTracking: false, explorerContextMenu: true } });
    expect(service.probe.reasons).toContain(tracking.reason); expect(service.explorer).toEqual(explorerState(true)); expect(service.cli).toEqual(cliState(true));
    expect(saveCli).not.toHaveBeenCalled(); expect(saveExplorer).not.toHaveBeenCalled(); expect(f.factory).not.toHaveBeenCalled();
  } finally { discovery.resolve(profiles); explorerDone.resolve(success(explorerState(false))); cliDone.resolve(success(cliState(false))); trackingDone.resolve(tracking); await initializing; await service.dispose(); }
});
it('reports failed integration repairs without skipping other checks or metadata migrations', async () => {
  const repository = new MemoryRepository(); repository.preference = false; Object.assign(repository, { cliPreference: () => false });
  const f = factoryFixture(), backend = new PtyBackend(f.options); vi.spyOn(backend, 'initialize').mockResolvedValue(failure('DEPENDENCY_MISSING', 'No PTY'));
  vi.mocked(getProcessTrackingCapability).mockResolvedValue({ available: false, reason: 'No tracking', helperPath: null });
  const explorer = { get: vi.fn(async () => success(explorerState(false))), set: vi.fn(async () => failure('AUTH_FAILED', 'Foreign verb')) };
  const cli = { binDir: '/fixture/bin', get: vi.fn(async () => success(cliState(false))), set: vi.fn(async () => failure('AUTH_FAILED', 'Foreign shim')) };
  const transaction = vi.spyOn(repository, 'transaction'), watch = vi.fn(), service = new EmbeddedSessionService(repository, backend, () => ({ setWatch: watch, dispose: vi.fn() }), explorer, cli);
  try {
    await service.initialize(); expect(service.probe.available).toBe(false); expect(service.probe.reasons).toEqual(expect.arrayContaining(['No PTY', 'No tracking']));
    expect(service.explorer).toMatchObject({ installed: false, reason: 'Foreign verb' }); expect(service.cli).toMatchObject({ installed: false, reason: 'Foreign shim' });
    expect(explorer.get).toHaveBeenCalledOnce(); expect(cli.get).toHaveBeenCalledOnce(); expect(explorer.set).toHaveBeenCalledWith(true); expect(cli.set).toHaveBeenCalledWith(true);
    expect(transaction).toHaveBeenCalledOnce(); expect(watch).toHaveBeenCalledWith([], expect.any(Array)); expect(repository.explorerPreference()).toBe(false); expect(f.factory).not.toHaveBeenCalled();
  } finally { await service.dispose(); }
});
it.each(['backend', 'explorer'] as const)('drains all startup work before propagating an unexpected %s rejection', async dependency => {
  Object.defineProperty(process, 'platform', { value: 'win32' });
  const repository = new MemoryRepository(), f = factoryFixture(), backend = new PtyBackend(f.options), error = new Error('Unexpected startup failure');
  if (dependency === 'backend') vi.spyOn(backend, 'initialize').mockRejectedValue(error);
  const cliDone = deferred<Result<CliIntegrationDto>>(), trackingDone = deferred<ProcessTrackingCapability>();
  const explorer = { get: vi.fn(async () => { if (dependency === 'explorer') throw error; return success(explorerState(true)); }), set: vi.fn(async () => success(explorerState(true))) };
  const cli = { binDir: '/fixture/bin', get: vi.fn(() => cliDone.promise), set: vi.fn(async () => success(cliState(true))) };
  vi.mocked(getProcessTrackingCapability).mockReturnValue(trackingDone.promise);
  const transaction = vi.spyOn(repository, 'transaction'), watch = vi.fn(), service = new EmbeddedSessionService(repository, backend, () => ({ setWatch: watch, dispose: vi.fn() }), explorer, cli);
  let finished = false; const initializing = service.initialize().then(() => { finished = true; return null; }, error => { finished = true; return error; });
  try {
    await flush(); expect(cli.get).toHaveBeenCalledOnce(); expect(finished).toBe(false); trackingDone.resolve({ available: true, reason: null, helperPath: null });
    await flush(); expect(finished).toBe(false); cliDone.resolve(success(cliState(true))); expect(await initializing).toBe(error); expect(transaction).not.toHaveBeenCalled(); expect(watch).not.toHaveBeenCalled();
  } finally { trackingDone.resolve({ available: true, reason: null, helperPath: null }); cliDone.resolve(success(cliState(true))); await initializing; await service.dispose(); }
});
it('completes readiness without integration ports and touches no desktop configuration', async () => {
  vi.mocked(getProcessTrackingCapability).mockResolvedValue({ available: true, reason: null, helperPath: null });
  const f = factoryFixture(), service = new EmbeddedSessionService(new MemoryRepository(), new PtyBackend(f.options), () => ({ setWatch: vi.fn(), dispose: vi.fn() }));
  try { await service.initialize(); expect(service.probe.available).toBe(true); expect(service.cli.installed).toBe(false); expect(service.explorer.installed).toBe(false); expect(f.factory).not.toHaveBeenCalled(); }
  finally { await service.dispose(); }
});
