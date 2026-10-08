import { afterEach, expect, it, vi } from 'vitest';
import { failure, success } from '../../shared/contracts';
import type { CliIntegrationDto, ExplorerIntegrationDto, Result, TerminalProfilesDto } from '../../shared/contracts';
import { PtyBackend } from './backend';
import { EmbeddedSessionService } from './service';
import { factoryFixture, MemoryRepository, profiles } from './test-fixtures';
import { getProcessTrackingCapability, type ProcessTrackingCapability } from './tracking';

vi.mock('./tracking', async importOriginal => ({
  ...await importOriginal<typeof import('./tracking')>(),
  getProcessTrackingCapability: vi.fn(),
}));
const platformDescriptor = Object.getOwnPropertyDescriptor(process, 'platform')!;
afterEach(() => { Object.defineProperty(process, 'platform', platformDescriptor); vi.restoreAllMocks(); vi.resetAllMocks(); });
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
}
const flush = () => new Promise<void>(resolve => setImmediate(resolve));

it.each([
  ['win32', true, 'backend'], ['win32', false, 'tracking'],
  ['linux', true, 'backend'], ['linux', false, 'tracking'],
] as const)('overlaps independent %s readiness work, opt-in %s, %s finishes first', async (platform, enabled, first) => {
  Object.defineProperty(process, 'platform', { value: platform });
  const repository = new MemoryRepository(); repository.preference = enabled;
  repository.config.adapterId = 'windows-terminal'; repository.config.pythonPath = '/saved/python';
  Object.assign(repository, { cliPreference: () => enabled });
  const discovery = deferred<TerminalProfilesDto>(), explorerDone = deferred<Result<ExplorerIntegrationDto>>();
  const cliDone = deferred<Result<CliIntegrationDto>>(), trackingDone = deferred<ProcessTrackingCapability>();
  const explorerState = { supported: true, installed: enabled, folderItemInstalled: enabled, backgroundInstalled: enabled, reason: null };
  const cliState = { supported: true, installed: enabled, command: 'shellfox start <path>', reason: null };
  const explorer = { get: vi.fn(() => explorerDone.promise), set: vi.fn(() => explorerDone.promise) };
  const cli = { binDir: '/fixture/bin', get: vi.fn(() => cliDone.promise), set: vi.fn(() => cliDone.promise) };
  const f = factoryFixture(), discover = vi.fn(() => discovery.promise);
  const backend = new PtyBackend({ ...f.options, discover });
  const configurePython = vi.spyOn(backend, 'configurePythonPath'), getProfiles = vi.spyOn(backend, 'getProfiles');
  const transaction = vi.spyOn(repository, 'transaction'), watch = vi.fn(), changed = vi.fn();
  vi.mocked(getProcessTrackingCapability).mockReturnValue(trackingDone.promise);
  const service = new EmbeddedSessionService(repository, backend, () => ({ setWatch: watch, dispose: vi.fn() }), explorer, cli);
  service.subscribe(changed);
  let ready = false;
  const initializing = service.initialize().then(() => { ready = true; });
  try {
    await flush();
    expect(configurePython).toHaveBeenCalledWith('/saved/python');
    expect(discover).toHaveBeenCalledOnce(); expect(getProcessTrackingCapability).toHaveBeenCalledOnce();
    expect(explorer[enabled ? 'set' : 'get']).toHaveBeenCalledOnce();
    expect(cli[enabled ? 'set' : 'get']).toHaveBeenCalledTimes(platform === 'linux' ? 0 : 1);
    expect(getProfiles).not.toHaveBeenCalled();
    const tracking = { available: false, reason: 'Tracking preflight failed', helperPath: null };
    if (first === 'backend') discovery.resolve(profiles); else trackingDone.resolve(tracking);
    await flush();
    expect(ready).toBe(false); expect(transaction).not.toHaveBeenCalled(); expect(watch).not.toHaveBeenCalled();
    explorerDone.resolve(success(explorerState)); await flush();
    expect(cli[enabled ? 'set' : 'get']).toHaveBeenCalledOnce();
    cliDone.resolve(success(cliState)); await flush();
    expect(ready).toBe(false); expect(changed).not.toHaveBeenCalled();
    expect(transaction).not.toHaveBeenCalled(); expect(watch).not.toHaveBeenCalled();
    if (first === 'backend') trackingDone.resolve(tracking); else discovery.resolve(profiles);
    await initializing;
    expect(ready).toBe(true); expect(transaction).toHaveBeenCalledOnce();
    expect(repository.settings().adapterId).toBe('embedded-pty');
    expect(watch).toHaveBeenCalledWith([], repository.settings().processRules);
    expect(service.probe).toMatchObject({ available: true, capabilities: { processTracking: false, explorerContextMenu: true } });
    expect(service.probe.reasons).toContain(tracking.reason);
    expect(service.explorer).toEqual(explorerState); expect(service.cli).toEqual(cliState);
    expect(explorer[enabled ? 'get' : 'set']).not.toHaveBeenCalled();
    expect(cli[enabled ? 'get' : 'set']).not.toHaveBeenCalled();
    if (enabled) { expect(explorer.set).toHaveBeenCalledWith(true); expect(cli.set).toHaveBeenCalledWith(true); }
    expect(f.factory).not.toHaveBeenCalled();
  } finally {
    discovery.resolve(profiles); explorerDone.resolve(success(explorerState)); cliDone.resolve(success(cliState));
    trackingDone.resolve({ available: true, reason: null, helperPath: null });
    await initializing; await service.dispose();
  }
});

it('reports backend/integration failures without skipping other checks or metadata migrations', async () => {
  const repository = new MemoryRepository(); repository.preference = true;
  Object.assign(repository, { cliPreference: () => true });
  const f = factoryFixture(), backend = new PtyBackend(f.options);
  vi.spyOn(backend, 'initialize').mockResolvedValue(failure('DEPENDENCY_MISSING', 'No PTY'));
  vi.mocked(getProcessTrackingCapability).mockResolvedValue({ available: false, reason: 'No tracking', helperPath: null });
  const explorer = { get: vi.fn(), set: vi.fn(async () => failure('AUTH_FAILED', 'Foreign verb')) };
  const cli = { binDir: '/fixture/bin', get: vi.fn(), set: vi.fn(async () => failure('AUTH_FAILED', 'Foreign shim')) };
  const transaction = vi.spyOn(repository, 'transaction'), watch = vi.fn();
  const service = new EmbeddedSessionService(repository, backend, () => ({ setWatch: watch, dispose: vi.fn() }), explorer, cli);
  try {
    await service.initialize();
    expect(service.probe.available).toBe(false); expect(service.probe.reasons).toEqual(expect.arrayContaining(['No PTY', 'No tracking']));
    expect(service.explorer).toMatchObject({ installed: false, reason: 'Foreign verb' });
    expect(service.cli).toMatchObject({ installed: false, reason: 'Foreign shim' });
    expect(explorer.set).toHaveBeenCalledWith(true); expect(cli.set).toHaveBeenCalledWith(true);
    expect(explorer.get).not.toHaveBeenCalled(); expect(cli.get).not.toHaveBeenCalled();
    expect(transaction).toHaveBeenCalledOnce(); expect(watch).toHaveBeenCalledWith([], expect.any(Array));
    expect(repository.explorerPreference()).toBe(true); expect(service.repository.cliPreference?.()).toBe(true);
    expect(f.factory).not.toHaveBeenCalled();
  } finally { await service.dispose(); }
});

it.each(['backend', 'explorer'] as const)('drains in-flight startup work before propagating an unexpected %s rejection', async dependency => {
  Object.defineProperty(process, 'platform', { value: 'win32' });
  const repository = new MemoryRepository(), f = factoryFixture(), backend = new PtyBackend(f.options);
  const error = new Error('Unexpected startup failure');
  if (dependency === 'backend') vi.spyOn(backend, 'initialize').mockRejectedValue(error);
  const cliDone = deferred<Result<CliIntegrationDto>>(), trackingDone = deferred<ProcessTrackingCapability>();
  const explorer = { get: vi.fn(async () => {
    if (dependency === 'explorer') throw error;
    return success({ supported: true, installed: false, folderItemInstalled: false, backgroundInstalled: false, reason: null });
  }), set: vi.fn() };
  const cli = { binDir: '/fixture/bin', get: vi.fn(() => cliDone.promise), set: vi.fn() };
  vi.mocked(getProcessTrackingCapability).mockReturnValue(trackingDone.promise);
  const transaction = vi.spyOn(repository, 'transaction'), watch = vi.fn();
  const service = new EmbeddedSessionService(repository, backend, () => ({ setWatch: watch, dispose: vi.fn() }), explorer, cli);
  let finished = false;
  const initializing = service.initialize().then(() => { finished = true; return null; }, error => { finished = true; return error; });
  try {
    await flush(); expect(cli.get).toHaveBeenCalledOnce(); expect(finished).toBe(false);
    trackingDone.resolve({ available: true, reason: null, helperPath: null });
    await flush(); expect(finished).toBe(false);
    cliDone.resolve(success({ supported: true, installed: false, command: 'shellfox start <path>', reason: null }));
    expect(await initializing).toBe(error);
    expect(transaction).not.toHaveBeenCalled(); expect(watch).not.toHaveBeenCalled();
  } finally {
    trackingDone.resolve({ available: true, reason: null, helperPath: null });
    cliDone.resolve(success({ supported: true, installed: false, command: 'shellfox start <path>', reason: null }));
    await initializing; await service.dispose();
  }
});

it('completes readiness without integration ports', async () => {
  vi.mocked(getProcessTrackingCapability).mockResolvedValue({ available: true, reason: null, helperPath: null });
  const f = factoryFixture(), service = new EmbeddedSessionService(new MemoryRepository(), new PtyBackend(f.options), () => ({ setWatch: vi.fn(), dispose: vi.fn() }));
  try { await service.initialize(); expect(service.probe.available).toBe(true); expect(f.factory).not.toHaveBeenCalled(); }
  finally { await service.dispose(); }
});
