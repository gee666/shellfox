import { expect, it, vi } from 'vitest';
import { success, failure, type Result, type ExplorerIntegrationDto, type CliIntegrationDto } from '../../shared/contracts';
import { PtyBackend } from './backend';
import { EmbeddedSessionService } from './service';
import { factoryFixture, MemoryRepository } from './test-fixtures';

it.each([true, false])('startup enables Explorer and CLI regardless of historical preference %s', async historical => {
  const repository = new MemoryRepository(); repository.preference = historical;
  const oldCliPreference = vi.fn(() => historical); Object.assign(repository, { cliPreference: oldCliPreference });
  const saveExplorer = vi.spyOn(repository, 'saveExplorerPreference');
  const explorerState: ExplorerIntegrationDto = { supported: true, installed: false, folderItemInstalled: false, backgroundInstalled: false, reason: null };
  const cliState: CliIntegrationDto = { supported: true, installed: false, command: 'shellfox', reason: null };
  const explorer = {
    get: vi.fn(async () => success(explorerState)),
    set: vi.fn<(enabled: boolean) => Promise<Result<ExplorerIntegrationDto>>>(async enabled => success({ ...explorerState, installed: enabled, folderItemInstalled: enabled, backgroundInstalled: enabled })),
  };
  const cli = {
    binDir: '/fixture/bin', get: vi.fn(async () => success(cliState)),
    set: vi.fn<(enabled: boolean) => Promise<Result<CliIntegrationDto>>>(async enabled => success({ ...cliState, installed: enabled })),
  };
  const f = factoryFixture();
  const service = new EmbeddedSessionService(repository, new PtyBackend(f.options), () => ({ setWatch: vi.fn(), dispose: vi.fn() }), explorer, cli);
  try {
    await service.initialize();
    expect(explorer.set).toHaveBeenCalledExactlyOnceWith(true); expect(cli.set).toHaveBeenCalledExactlyOnceWith(true);
    expect(service.explorer.installed).toBe(true); expect(service.cli.installed).toBe(true);
    expect(oldCliPreference).not.toHaveBeenCalled(); expect(saveExplorer).not.toHaveBeenCalled();
    explorer.set.mockResolvedValueOnce(failure('AUTH_FAILED', 'Foreign owner'));
    expect(await service.setExplorerIntegration({ installed: true })).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
    expect(await service.setExplorerIntegration({ installed: false })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    expect(await service.setCliIntegration({ installed: false })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    expect(repository.explorerPreference()).toBe(historical);
    expect(f.factory).not.toHaveBeenCalled();
  } finally { await service.dispose(); }
});
