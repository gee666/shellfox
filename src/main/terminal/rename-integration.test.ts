import { expect, it, vi } from 'vitest';
import { success, failure } from '../../shared/contracts';
import { PtyBackend } from './backend';
import { EmbeddedSessionService } from './service';
import { factoryFixture, MemoryRepository } from './test-fixtures';

it.each([true, false])('startup preserves saved Explorer/CLI opt-in %s and refreshes enabled commands', async enabled => {
  const repository = new MemoryRepository(); repository.preference = enabled;
  Object.assign(repository, { cliPreference: () => enabled });
  const explorerState = { supported: true, installed: enabled, folderItemInstalled: enabled, backgroundInstalled: enabled, reason: null };
  const cliState = { supported: true, installed: enabled, command: 'shellfox start <path>', reason: null };
  const explorer = { get: vi.fn(async () => success(explorerState)), set: vi.fn(async (_enabled: boolean) => success(explorerState)) };
  const cli = { binDir: '/fixture/bin', get: vi.fn(async () => success(cliState)), set: vi.fn(async (_enabled: boolean) => success(cliState)) };
  const f = factoryFixture();
  const service = new EmbeddedSessionService(repository, new PtyBackend(f.options), () => ({ setWatch: vi.fn(), dispose: vi.fn() }), explorer, cli);
  try {
    await service.initialize();
    expect(explorer.set).toHaveBeenCalledTimes(enabled ? 1 : 0);
    expect(explorer.get).toHaveBeenCalledTimes(enabled ? 0 : 1);
    expect(cli.set).toHaveBeenCalledTimes(enabled ? 1 : 0);
    expect(cli.get).toHaveBeenCalledTimes(enabled ? 0 : 1);
    if (enabled) { expect(explorer.set).toHaveBeenCalledWith(true); expect(cli.set).toHaveBeenCalledWith(true); }
    explorer.set.mockResolvedValueOnce(failure('AUTH_FAILED', 'Foreign owner') as never);
    expect(await service.setExplorerIntegration({ installed: !enabled })).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
    expect(repository.explorerPreference()).toBe(enabled);
    expect(f.factory).not.toHaveBeenCalled();
  } finally { await service.dispose(); }
});
