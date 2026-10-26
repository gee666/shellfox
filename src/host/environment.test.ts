import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { afterEach, expect, it, vi } from 'vitest';
import { terminalEnvironment } from '../main/terminal/environment';
import { bundledHostCli } from './cli-integration';
import { hostResourcePaths } from './paths';
import { EmbeddedSessionService } from '../main/terminal/service';
import { PtyBackend } from '../main/terminal/backend';
import { factoryFixture, MemoryRepository } from '../main/terminal/test-fixtures';
import { success, failure } from '../shared/contracts';

const directories: string[] = [];
afterEach(async () => { for (const directory of directories.splice(0)) await rm(directory, { recursive: true, force: true }); });
it('pins terminal CLI identity to its own host without leaking inherited foreign hosts', () => {
  const own = { userData: '/own/data', runtimePath: '/own/runtime', resourcesPath: '/own/resources', headless: true };
  const env = terminalEnvironment({ SHELLFOX_USER_DATA: '/foreign/data', SHELLFOX_RUNTIME_PATH: '/foreign/runtime', PATH: '/bin' }, [], { windows: false, wsl: false, host: own, cliBin: own.runtimePath });
  expect(env.SHELLFOX_USER_DATA).toBe(own.userData); expect(env.SHELLFOX_RUNTIME_PATH).toBe(own.runtimePath);
  expect(env.SHELLFOX_RESOURCES_PATH).toBe(own.resourcesPath); expect(env.SHELLFOX_HEADLESS).toBe('1');
  expect(env.PATH).toBe('/own/runtime:/bin');
  const second = terminalEnvironment(env, [], { windows: false, wsl: false, host: { userData: '/second/data' } });
  expect(second.SHELLFOX_USER_DATA).toBe('/second/data'); expect(second.SHELLFOX_RUNTIME_PATH).toBeUndefined();
});
it('keeps an Electron-as-Node host launcher flag out of ordinary terminal programs', () => {
  const host = { userData: '/host/data', headless: true };
  const inherited = { ELECTRON_RUN_AS_NODE: '1', PATH: '/bin' };
  expect(terminalEnvironment(inherited, [], { windows: false, wsl: false, host }).ELECTRON_RUN_AS_NODE).toBeUndefined();
  expect(terminalEnvironment(inherited, [{ name: 'ELECTRON_RUN_AS_NODE', value: '1' }], { windows: false, wsl: false, host }).ELECTRON_RUN_AS_NODE).toBe('1');
});
it('transfers trusted runtime paths through WSLENV with path translation', () => {
  const env = terminalEnvironment({ WSLENV: 'SHELLFOX_USER_DATA/u:OTHER/u' }, [], { windows: true, wsl: true, host: { userData: 'C:\\data', runtimePath: 'C:\\runtime', headless: true } });
  expect(env.WSLENV).toContain('SHELLFOX_USER_DATA/p'); expect(env.WSLENV).toContain('SHELLFOX_RUNTIME_PATH/p');
  expect(env.WSLENV).not.toContain('SHELLFOX_USER_DATA/u'); expect(env.WSLENV).toContain('OTHER/u');
});
it('advertises the bundled CLI only when launcher, Node and CLI bundle all exist', async () => {
  await mkdir('tmp', { recursive: true }); const root = await mkdtemp(path.resolve('tmp/host-cli-')); directories.push(root);
  expect(await bundledHostCli(root)).toBeUndefined(); await mkdir(path.join(root, 'bin')); await mkdir(path.join(root, 'app'));
  const launcher = process.platform === 'win32' ? 'shellfox.cmd' : 'shellfox';
  await writeFile(path.join(root, launcher), '', { mode: 0o755 });
  await writeFile(path.join(root, 'bin', process.platform === 'win32' ? 'node.exe' : 'node'), '', { mode: 0o755 });
  expect(await bundledHostCli(root)).toBeUndefined(); await writeFile(path.join(root, 'app/main.cjs'), '');
  const cli = await bundledHostCli(root); expect(cli?.binDir).toBe(root);
  expect(await cli?.get()).toMatchObject({ ok: true, value: { installed: true, supported: false } });
  expect(await cli?.set(false)).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
});
it('does not replay a desktop global-PATH preference into a read-only host CLI', async () => {
  const fixture = factoryFixture(), set = vi.fn(async () => failure('UNSUPPORTED', 'read-only host CLI'));
  const repository = Object.assign(new MemoryRepository(), { cliPreference: () => true });
  const cli = { binDir: process.cwd(), readOnly: true as const, get: async () => success({ supported: false, installed: true, command: 'shellfox', reason: 'Private host CLI' }), set };
  const service = new EmbeddedSessionService(repository, new PtyBackend(fixture.options), () => ({ setWatch() {}, dispose() {} }), undefined, cli);
  try { await service.initialize(); expect(service.cli.installed).toBe(true); expect(set).not.toHaveBeenCalled(); }
  finally { await service.dispose(); }
});
it('uses explicit packaged resource paths without Electron globals', () => {
  const paths = hostResourcePaths({ packaged: true, resourcesPath: path.resolve('tmp/resources') });
  expect(paths.helpers.resourcesPath).toBe(path.resolve('tmp/resources'));
  expect(paths.tailscale).toContain(path.join('tmp', 'resources', 'tailscale'));
});
