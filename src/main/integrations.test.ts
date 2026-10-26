import { expect, it, vi } from 'vitest';
import type { CliIntegrationDto, ExplorerIntegrationDto } from '../shared/contracts';
import { success, failure } from '../shared/contracts';
import { DesktopIntegrations } from './integrations';

const cliState = (installed: boolean, supported = true): CliIntegrationDto => ({ installed, supported, command: 'shellfox', reason: supported ? null : 'Unsupported CLI' });
const folderState = (installed: boolean, supported = true): ExplorerIntegrationDto => ({ installed, supported, folderItemInstalled: installed, backgroundInstalled: installed, reason: supported ? null : 'Finder integration is unsupported' });
it('keeps a read-only headless CLI enabled without global PATH or file-manager writes', async () => {
  const cli = { readOnly: true as const, get: vi.fn(async () => success(cliState(true, false))), set: vi.fn(async () => failure('UNSUPPORTED', 'Read-only CLI')) };
  const integrations = new DesktopIntegrations(undefined, cli, () => true, vi.fn()); await integrations.initialize(); integrations.markReady();
  expect(await integrations.ensureCli()).toMatchObject({ ok: true, value: { installed: true, supported: false } });
  await integrations.refresh(); expect(cli.set).not.toHaveBeenCalled(); expect(integrations.explorer.installed).toBe(false);
  expect(await integrations.ensureExplorer()).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
});
it('does not fake Finder support or install an unsupported provider', async () => {
  const explorer = { get: vi.fn(async () => success(folderState(false, false))), set: vi.fn(async () => success(folderState(true))) };
  const integrations = new DesktopIntegrations(explorer, undefined, () => true, vi.fn(), 'darwin'); await integrations.initialize(); integrations.markReady();
  expect(integrations.explorer).toMatchObject({ installed: false, supported: false });
  expect(await integrations.ensureExplorer()).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } }); expect(explorer.set).not.toHaveBeenCalled();
});
it('preserves failed ownership checks without claiming installation succeeded', async () => {
  const explorer = { get: vi.fn(async () => success(folderState(false))), set: vi.fn(async () => failure('AUTH_FAILED', 'Foreign owner')) };
  const integrations = new DesktopIntegrations(explorer, undefined, () => true, vi.fn()); await integrations.initialize(); integrations.markReady(); await integrations.refresh();
  expect(integrations.explorer).toMatchObject({ installed: false, supported: true, reason: 'Foreign owner' }); expect(explorer.set).toHaveBeenCalledExactlyOnceWith(true);
});
it('does not install the Linux CLI twice when file-manager setup already installed its dependency', async () => {
  let cliInstalled = false, folderInstalled = false;
  const cli = { get: vi.fn(async () => success(cliState(cliInstalled))), set: vi.fn(async (installed: boolean) => { cliInstalled = installed; return success(cliState(installed)); }) };
  const explorer = { get: vi.fn(async () => success(folderState(folderInstalled))), set: vi.fn(async (installed: boolean) => { await cli.set(installed); folderInstalled = installed; return success(folderState(installed)); }) };
  const integrations = new DesktopIntegrations(explorer, cli, () => true, vi.fn(), 'linux'); await integrations.initialize(); integrations.markReady();
  expect(cli.set).toHaveBeenCalledExactlyOnceWith(true); expect(explorer.set).toHaveBeenCalledExactlyOnceWith(true);
  await integrations.ensureCli(); await integrations.ensureExplorer(); expect(cli.set).toHaveBeenCalledOnce(); expect(explorer.set).toHaveBeenCalledOnce();
});
it('serializes Linux menu and CLI writes so repairs never inspect each other mid-write', async () => {
  let cliInstalled = true, folderInstalled = true, writing = false; let finish: (() => void) | undefined;
  const pending = new Promise<void>(resolve => { finish = resolve; });
  const cli = { get: vi.fn(async () => { expect(writing).toBe(false); return success(cliState(cliInstalled)); }), set: vi.fn(async () => { expect(writing).toBe(false); cliInstalled = true; return success(cliState(true)); }) };
  const explorer = { get: vi.fn(async () => success(folderState(folderInstalled))), set: vi.fn(async () => { writing = true; await pending; writing = false; folderInstalled = true; return success(folderState(true)); }) };
  const integrations = new DesktopIntegrations(explorer, cli, () => true, vi.fn(), 'linux'); await integrations.initialize(); integrations.markReady(); folderInstalled = false; cliInstalled = false;
  const folderRepair = integrations.ensureExplorer(); await vi.waitFor(() => expect(writing).toBe(true)); const cliRepair = integrations.ensureCli();
  try { await Promise.resolve(); expect(cli.get).toHaveBeenCalledOnce(); }
  finally { finish?.(); await Promise.all([folderRepair, cliRepair]); }
  expect(cli.set).toHaveBeenCalledExactlyOnceWith(true); expect(integrations.cli.installed).toBe(true);
});
