import { ChildProcess, execFile } from 'node:child_process';
import * as childProcess from 'node:child_process';
import { chmod, copyFile, mkdir, mkdtemp, readFile, rm, unlink, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { promisify } from 'node:util';
import { build } from 'esbuild';
import { afterEach, expect, it, vi } from 'vitest';
import { requiredHeadlessExecutables, requiredHeadlessFiles } from './headless-installation';
import { acquireUpdateLock, activateHeadlessUpdate, activeHeadlessInstallation, forwardHeadlessUpdate, headlessUpdateAnchor, UPDATE_POINTER } from './update-activation';

vi.mock('node:child_process', async importOriginal => ({ ...await importOriginal<typeof import('node:child_process')>(), spawn: vi.fn() }));
const cleanup: string[] = [];
afterEach(async () => { vi.restoreAllMocks(); for (const root of cleanup.splice(0)) await rm(root, { recursive: true, force: true }); });
async function packageFixture(root: string, version: string) {
  for (const name of requiredHeadlessFiles(process.platform)) {
    await mkdir(path.dirname(path.join(root, name)), { recursive: true });
    await writeFile(path.join(root, name), 'mock file, not executable');
  }
  for (const name of requiredHeadlessExecutables(process.platform)) await chmod(path.join(root, name), 0o755);
  await writeFile(path.join(root, 'runtime.json'), JSON.stringify({ version: 1, node: process.platform === 'win32' ? 'bin/node.exe' : 'bin/node', entry: 'app/main.cjs', resources: 'resources', runAsNode: false, headless: true }));
  await writeFile(path.join(root, 'build.json'), JSON.stringify({ product: 'headless', version, platform: process.platform, arch: process.arch }));
}
async function fixture() {
  await mkdir('tmp', { recursive: true });
  const parent = await mkdtemp(path.resolve('tmp/update activation ')); cleanup.push(parent);
  const anchor = path.join(parent, 'original'); await packageFixture(anchor, '0.9.5');
  async function version(version: string) {
    const directory = await mkdtemp(path.join(parent, `shellfox-headless-${version}-${process.platform}-${process.arch}-`));
    const installation = path.join(directory, 'installation', `shellfox-headless-${process.platform}-${process.arch}`);
    await packageFixture(installation, version); return installation;
  }
  return { parent, anchor, version };
}
it('atomically selects the new package without changing the old entry, runtime, or service paths', async () => {
  const f = await fixture(), target = await f.version('0.9.6');
  const original = await readFile(path.join(f.anchor, 'app/main.cjs'), 'utf8');
  await activateHeadlessUpdate(f.anchor, target);
  expect(await activeHeadlessInstallation(f.anchor)).toBe(target);
  expect(await activeHeadlessInstallation(target)).toBeNull();
  expect(await headlessUpdateAnchor(target)).toBe(f.anchor);
  expect(await readFile(path.join(f.anchor, 'app/main.cjs'), 'utf8')).toBe(original);
});
it('repeated updates all use one stable pointer, including commands from old version paths', async () => {
  const f = await fixture(), first = await f.version('0.9.6'), second = await f.version('0.9.7');
  await activateHeadlessUpdate(f.anchor, first);
  await activateHeadlessUpdate(await headlessUpdateAnchor(first), second);
  expect(await activeHeadlessInstallation(f.anchor)).toBe(second);
  expect(await activeHeadlessInstallation(first)).toBe(second);
  expect(await activeHeadlessInstallation(second)).toBeNull();
  // Retaining the previous version also permits pointer-only rollback without replacing live code.
  await writeFile(path.join(f.anchor, UPDATE_POINTER), JSON.stringify({ version: 1, installation: first }));
  expect(await activeHeadlessInstallation(f.anchor)).toBe(first);
});
it('leaves the previous pointer intact if activation fails before commit', async () => {
  const f = await fixture(), first = await f.version('0.9.6'), second = await f.version('0.9.7');
  await activateHeadlessUpdate(f.anchor, first);
  await writeFile(path.join(second, 'headless-update-origin.json'), 'already exists');
  await expect(activateHeadlessUpdate(f.anchor, second)).rejects.toMatchObject({ code: 'EEXIST' });
  expect(await activeHeadlessInstallation(f.anchor)).toBe(first);
});
it('serializes installation and releases the lock for a retry', async () => {
  const f = await fixture(), release = await acquireUpdateLock(f.anchor);
  await expect(acquireUpdateLock(f.anchor)).rejects.toThrow('locked');
  await release();
  await (await acquireUpdateLock(f.anchor))();
});
it.each(['malformed', 'external', 'missing', 'wrong-origin', 'native-missing'])('fails closed on a %s activated package', async kind => {
  const f = await fixture(), target = await f.version('0.9.6');
  await activateHeadlessUpdate(f.anchor, target);
  if (kind === 'malformed') await writeFile(path.join(f.anchor, UPDATE_POINTER), '{');
  if (kind === 'external') await writeFile(path.join(f.anchor, UPDATE_POINTER), JSON.stringify({ version: 1, installation: f.parent }));
  if (kind === 'missing') await rm(target, { recursive: true });
  if (kind === 'wrong-origin') await writeFile(path.join(target, 'headless-update-origin.json'), JSON.stringify({ version: 1, anchor: f.parent }));
  if (kind === 'native-missing') await unlink(path.join(target, 'app/node_modules/node-pty/build/Release/pty.node'));
  await expect(activeHeadlessInstallation(f.anchor)).rejects.toThrow();
});
it('forwards argv and inherited stdio to the selected bundled Node, then returns its exit status', async () => {
  const f = await fixture(), target = await f.version('0.9.6'); await activateHeadlessUpdate(f.anchor, target);
  const child = new ChildProcess();
  const spawn = vi.mocked(childProcess.spawn).mockImplementation(() => { queueMicrotask(() => child.emit('close', 17, null)); return child; });
  const before = process.listenerCount('SIGTERM');
  expect(await forwardHeadlessUpdate(['--user-data', 'path with spaces', 'host', 'run', '--json'], path.join(f.anchor, 'app/main.cjs'))).toBe(17);
  expect(spawn).toHaveBeenCalledWith(path.join(target, 'bin', process.platform === 'win32' ? 'node.exe' : 'node'), [path.join(target, 'app/main.cjs'), '--user-data', 'path with spaces', 'host', 'run', '--json'], expect.objectContaining({ stdio: ['inherit', 'inherit', 'inherit', 'ipc'], shell: false }));
  expect(process.listenerCount('SIGTERM')).toBe(before);
});
it('propagates child startup errors and removes signal listeners', async () => {
  const f = await fixture(), target = await f.version('0.9.6'); await activateHeadlessUpdate(f.anchor, target);
  const child = new ChildProcess();
  vi.mocked(childProcess.spawn).mockImplementation(() => { queueMicrotask(() => child.emit('error', new Error('Node failed'))); return child; });
  const before = process.listenerCount('SIGTERM');
  await expect(forwardHeadlessUpdate(['version'], path.join(f.anchor, 'app/main.cjs'))).rejects.toThrow('Node failed');
  expect(process.listenerCount('SIGTERM')).toBe(before);
});
it('the original packaged entry launches the activated CLI version with a real child process', async () => {
  const f = await fixture(), target = await f.version('0.9.6');
  for (const [root, version] of [[f.anchor, '0.9.5'], [target, '0.9.6']]) {
    await build({ entryPoints: ['src/main/platform/shellfox-cli-entry.ts'], outfile: path.join(root, 'app/main.cjs'), bundle: true, platform: 'node', format: 'cjs', packages: 'external', loader: { '.sh': 'text' }, define: { __SHELLFOX_VERSION__: JSON.stringify(version) } });
  }
  const node = path.join(target, 'bin', process.platform === 'win32' ? 'node.exe' : 'node');
  await unlink(node); await copyFile(process.execPath, node);
  await activateHeadlessUpdate(f.anchor, target);
  const result = await promisify(execFile)(process.execPath, [path.join(f.anchor, 'app/main.cjs'), '--json', 'version'], { timeout: 15_000 });
  expect(JSON.parse(result.stdout)).toEqual({ ok: true, value: '0.9.6' });
  expect(result.stderr).toBe('');
}, 25_000);
it('does not orphan a forwarded fixture when its forwarding process dies', async () => {
  const f = await fixture(), target = await f.version('0.9.6');
  const helper = path.join(f.parent, 'activation.cjs'), ready = path.join(f.parent, 'ready.json'), stopped = path.join(f.parent, 'stopped');
  await build({ entryPoints: ['src/cli/update-activation.ts'], outfile: helper, bundle: true, platform: 'node', format: 'cjs', packages: 'external' });
  const node = path.join(target, 'bin', process.platform === 'win32' ? 'node.exe' : 'node');
  await unlink(node); await copyFile(process.execPath, node);
  await writeFile(path.join(target, 'app/main.cjs'), `const fs = require('node:fs');
require(${JSON.stringify(helper)}).watchUpdateParent();
process.on('SIGTERM', () => { fs.writeFileSync(${JSON.stringify(stopped)}, 'graceful'); process.exit(0); });
fs.writeFileSync(${JSON.stringify(ready)}, JSON.stringify({ pid: process.pid, connected: process.connected, disconnectListeners: process.listenerCount('disconnect'), termListeners: process.listenerCount('SIGTERM') }));
setInterval(() => {}, 1000);
`);
  await activateHeadlessUpdate(f.anchor, target);
  const code = `require(${JSON.stringify(helper)}).forwardHeadlessUpdate([], ${JSON.stringify(path.join(f.anchor, 'app/main.cjs'))}).then(code => { process.exitCode = code; });`;
  const proxy = execFile(process.execPath, ['-e', code], { timeout: 10_000 });
  let pid: number | undefined;
  async function wait(file: string): Promise<string> {
    const end = Date.now() + 5000;
    while (Date.now() < end) {
      try { return await readFile(file, 'utf8'); }
      catch (error) { if (!(error instanceof Error) || !('code' in error) || error.code !== 'ENOENT') throw error; }
      await new Promise(resolve => setTimeout(resolve, 20));
    }
    throw new Error('Fixture lifetime deadline exceeded: ' + file + '; readiness: ' + await readFile(ready, 'utf8').catch(() => 'none'));
  }
  try {
    const value: unknown = JSON.parse(await wait(ready));
    if (!value || typeof value !== 'object' || !('pid' in value) || typeof value.pid !== 'number') throw new Error('Invalid fixture PID.');
    pid = value.pid;
    proxy.kill('SIGKILL');
    if (process.platform === 'win32') {
      // libuv's Windows job ends the process tree when its owner dies, before IPC cleanup can run.
      const end = Date.now() + 5000;
      let running = true;
      while (running && Date.now() < end) {
        try { process.kill(pid, 0); } catch { running = false; }
        if (running) await new Promise(resolve => setTimeout(resolve, 20));
      }
      expect(running).toBe(false);
    } else expect(await wait(stopped)).toBe('graceful');
    pid = undefined;
  } finally {
    proxy.kill('SIGKILL');
    if (pid) { try { process.kill(pid, 'SIGKILL'); } catch { /* The fixture already stopped. */ } }
  }
}, 15_000);
