import { constants } from 'node:fs';
import { access, chmod, mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { afterEach, expect, it, vi } from 'vitest';
import { readHeadlessInstallation, requiredHeadlessExecutables, requiredHeadlessFiles } from './headless-installation';
vi.mock('node:fs/promises', async importOriginal => {
  const actual = await importOriginal<typeof import('node:fs/promises')>();
  return { ...actual, access: vi.fn(actual.access) };
});
const cleanup: string[] = [];
afterEach(async () => { vi.unstubAllGlobals(); vi.mocked(access).mockRestore(); for (const root of cleanup.splice(0)) await rm(root, { recursive: true, force: true }); });
async function fixture(platform: string) {
  await mkdir('tmp', { recursive: true });
  const root = await mkdtemp(path.resolve('tmp/headless executable ')); cleanup.push(root);
  for (const name of requiredHeadlessFiles(platform)) {
    await mkdir(path.dirname(path.join(root, name)), { recursive: true });
    await writeFile(path.join(root, name), 'fixture, never executed');
  }
  for (const name of requiredHeadlessExecutables(platform)) await chmod(path.join(root, name), 0o755);
  await writeFile(path.join(root, 'build.json'), JSON.stringify({ product: 'headless', version: '0.9.5', platform, arch: 'x64' }));
  await writeFile(path.join(root, 'runtime.json'), JSON.stringify({ version: 1, node: platform === 'win32' ? 'bin/node.exe' : 'bin/node', entry: 'app/main.cjs', resources: 'resources', runAsNode: false, headless: true }));
  return root;
}
const executables = ['linux', 'darwin'].flatMap(platform => requiredHeadlessExecutables(platform).map(file => ({ platform, file })));
it.each(executables)('rejects a non-executable $platform/$file before accepting the installation', async ({ platform, file }) => {
  const root = await fixture(platform);
  // Exercise Unix permission-check failures on Windows CI too, where chmod/X_OK cannot enforce Unix modes.
  vi.stubGlobal('process', { ...process, platform: 'linux' });
  vi.mocked(access).mockImplementation(async (candidate, mode) => {
    if (String(candidate) === path.join(root, file) && mode === constants.X_OK) throw new Error('EACCES');
  });
  await expect(readHeadlessInstallation(root)).rejects.toThrow(`Headless package executable is not runnable: ${file}`);
});
it.each(['linux', 'darwin'])('checks execution access for every required %s executable', async platform => {
  const root = await fixture(platform);
  vi.stubGlobal('process', { ...process, platform: 'linux' });
  await expect(readHeadlessInstallation(root)).resolves.toMatchObject({ platform });
  for (const name of requiredHeadlessExecutables(platform)) expect(access).toHaveBeenCalledWith(path.join(root, name), constants.X_OK);
});
it('does not apply Unix executable checks on Windows', async () => {
  const root = await fixture('linux');
  vi.stubGlobal('process', { ...process, platform: 'win32' });
  await expect(readHeadlessInstallation(root)).resolves.toMatchObject({ platform: 'linux' });
  expect(vi.mocked(access).mock.calls.some(([, mode]) => mode === constants.X_OK)).toBe(false);
});
it.each([
  ['win32', 'service/run-host.ps1'], ['linux', 'service/shellfox-user.service.in'], ['darwin', 'service/com.shellfox.host.plist.in'],
])('rejects a %s update missing its startup file %s', async (platform, file) => {
  const root = await fixture(platform);
  await rm(path.join(root, file));
  await expect(readHeadlessInstallation(root)).rejects.toMatchObject({ code: 'ENOENT' });
});
it.skipIf(process.platform === 'win32')('rejects an actual Unix runtime after its execute permission is removed', async () => {
  const root = await fixture(process.platform);
  await chmod(path.join(root, 'bin/node'), 0o644);
  await expect(readHeadlessInstallation(root)).rejects.toThrow('Headless package executable is not runnable: bin/node');
});
