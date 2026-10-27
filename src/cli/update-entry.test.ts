import { afterEach, expect, it, vi } from 'vitest';
import { runShellfoxCli } from '../main/platform/shellfox-cli-entry';
import { runHeadlessUpdate } from './update';
import { resolveRuntimeContext } from './runtime-context';
import { dispatchDesktopLaunch } from '../main/platform/shellfox-cli-launch';
vi.mock('./update', async importOriginal => ({ ...await importOriginal<typeof import('./update')>(), runHeadlessUpdate: vi.fn() }));
vi.mock('./runtime-context', async importOriginal => ({ ...await importOriginal<typeof import('./runtime-context')>(), resolveRuntimeContext: vi.fn() }));
vi.mock('../main/platform/shellfox-cli-launch', () => ({ dispatchDesktopLaunch: vi.fn() }));
afterEach(() => { vi.restoreAllMocks(); vi.resetAllMocks(); vi.unstubAllGlobals(); vi.unstubAllEnvs(); });
async function invoke(args: string[], headless = true) {
  vi.stubGlobal('__SHELLFOX_VERSION__', '0.9.5');
  vi.mocked(resolveRuntimeContext).mockResolvedValue({ headless });
  vi.mocked(runHeadlessUpdate).mockResolvedValue({ current: '0.9.5', latest: '0.9.6', available: true, phase: 'available', installation: null, restartRequired: false, message: 'Shellfox 0.9.6 is available.' });
  let stdout = '', stderr = '';
  vi.spyOn(process.stdout, 'write').mockImplementation(value => { stdout += String(value); return true; });
  vi.spyOn(process.stderr, 'write').mockImplementation(value => { stderr += String(value); return true; });
  const code = await runShellfoxCli(args);
  return { code, stdout, stderr };
}
it.each([['update'], ['update', '--check'], ['--json', 'update', '--check'], ['update', '--check', '--json'], ['--host', 'local', 'update']])('dispatches headless update without connecting to a host: %j', async (...args) => {
  const result = await invoke(args);
  expect(result.code).toBe(0); expect(result.stderr).toBe('');
  expect(runHeadlessUpdate).toHaveBeenCalledWith({ entry: expect.any(String), current: '0.9.5', check: args.includes('--check') });
  expect(dispatchDesktopLaunch).not.toHaveBeenCalled();
  if (args.includes('--json')) expect(JSON.parse(result.stdout)).toMatchObject({ ok: true, value: { phase: 'available', restartRequired: false } });
  else expect(result.stdout).toContain('Shellfox 0.9.6 is available.');
});
it.each([['update', '--force'], ['update', '--check', '--check'], ['--host', 'remote-machine', 'update']])('rejects %j before update dispatch', async (...args) => {
  const result = await invoke(['--json', ...args]);
  expect(result.code).toBe(2); expect(JSON.parse(result.stdout).error.code).toBe('VALIDATION');
  expect(runHeadlessUpdate).not.toHaveBeenCalled(); expect(dispatchDesktopLaunch).not.toHaveBeenCalled();
});
it.each([['help', 'update'], ['update', '--help'], ['update', '-h']])('shows headless update help without downloading: %j', async (...args) => {
  const result = await invoke(args);
  expect(result.code).toBe(0); expect(result.stdout).toContain('installs a verified version');
  expect(result.stdout).toContain('never stop hosts'); expect(runHeadlessUpdate).not.toHaveBeenCalled();
});
it.each([
  ['update'], ['update', '--check'], ['--host', 'local', 'update', '--check'],
  ['update', '--host=local', '--check'], ['--yes', 'update', '--check'],
  ['--user-data', 'private data', 'update', '--check'],
])('dispatches canonical desktop update arguments after parsing globals: %j', async (...args) => {
  vi.mocked(dispatchDesktopLaunch).mockResolvedValue(0);
  const result = await invoke(args, false);
  expect(result.code).toBe(0); expect(dispatchDesktopLaunch).toHaveBeenCalledWith(['update', ...(args.includes('--check') ? ['--check'] : [])]);
  expect(runHeadlessUpdate).not.toHaveBeenCalled();
});
it.each([
  ['--host', 'remote-machine', 'update'], ['update', '--check', '--host=remote-machine'],
  ['update', '--json'], ['--json', 'update', '--check'], ['update', '--check', '--check'],
])('rejects remote, JSON, or invalid desktop update requests before dispatch: %j', async (...args) => {
  const result = await invoke(args, false);
  expect(result.code).toBe(2);
  if (args.includes('--json')) expect(JSON.parse(result.stdout).error.code).toBe('VALIDATION');
  else expect(result.stderr).toMatch(/local installation|Usage:/);
  expect(runHeadlessUpdate).not.toHaveBeenCalled(); expect(dispatchDesktopLaunch).not.toHaveBeenCalled();
});
it('returns an actionable unsupported error instead of unknown command for desktop CLI-only layouts', async () => {
  vi.mocked(dispatchDesktopLaunch).mockResolvedValue(null);
  const result = await invoke(['update'], false);
  expect(result.code).toBe(1); expect(result.stderr).toContain('in-app updater'); expect(result.stderr).not.toContain('Unknown command');
});
