import path from 'node:path';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { afterEach, expect, it, vi } from 'vitest';
import { applyRuntimeContext, resolveRuntimeContext } from './runtime-context';
const roots: string[] = [];
afterEach(async () => { vi.unstubAllEnvs(); for (const root of roots.splice(0)) await rm(root, { force: true, recursive: true }); });
async function fixture() { await mkdir('tmp', { recursive: true }); const root = await mkdtemp(path.resolve('tmp/cli-context-')); roots.push(root); return root; }
it.each(['win32', 'linux', 'darwin'])('infers %s desktop resources from its own manifest rather than cwd or foreign environment', async platform => {
  const root = await fixture(), cli = path.join(root, 'resources', 'cli'); await mkdir(cli, { recursive: true });
  await writeFile(path.join(cli, 'runtime.json'), JSON.stringify({ version: 1, node: platform === 'win32' ? 'cli-runtime/node.exe' : platform === 'darwin' ? '../../MacOS/Shellfox' : '../../shellfox', entry: 'main/cli.cjs', resources: '..', runAsNode: platform !== 'win32', headless: false }));
  expect(await resolveRuntimeContext(path.join(cli, 'main', 'cli.cjs'), { SHELLFOX_RESOURCES_PATH: '/foreign/resources', SHELLFOX_RUNTIME_PATH: '/foreign/runtime', SHELLFOX_PROJECT_ROOT: '/foreign/project', SHELLFOX_HEADLESS: '1' })).toEqual({ resourcesPath: path.join(root, 'resources'), runtimePath: undefined, headless: false });
});
it('infers headless resources and runtime root, including no-GUI behavior', async () => {
  const root = await fixture();
  await writeFile(path.join(root, 'runtime.json'), JSON.stringify({ version: 1, node: 'bin/node', entry: 'app/main.cjs', resources: 'resources', runAsNode: false, headless: true }));
  expect(await resolveRuntimeContext(path.join(root, 'app', 'main.cjs'), { SHELLFOX_HEADLESS: '0' })).toEqual({ resourcesPath: path.join(root, 'resources'), runtimePath: root, headless: true });
});
it('retains explicit development project context when no packaged manifest exists', async () => {
  const root = await fixture();
  expect(await resolveRuntimeContext(path.join(root, 'main', 'cli.cjs'), { SHELLFOX_PROJECT_ROOT: root })).toEqual({ projectRoot: root, resourcesPath: undefined, runtimePath: undefined, headless: false });
});
it.each([
  { version: 1, node: '/absolute/node', entry: 'main/cli.cjs', resources: '..', runAsNode: false, headless: false },
  { version: 1, node: 'node', entry: '../other.cjs', resources: '..', runAsNode: false, headless: false },
  { version: 1, node: 'node', entry: 'main/cli.cjs', resources: '/foreign', runAsNode: false, headless: false },
  { version: 1, node: 'node', entry: 'main/cli.cjs', resources: '../../foreign', runAsNode: false, headless: false },
])('rejects a mismatched or unsafe manifest %j', async manifest => {
  const root = await fixture(); await writeFile(path.join(root, 'runtime.json'), JSON.stringify(manifest));
  await expect(resolveRuntimeContext(path.join(root, 'main', 'cli.cjs'), {})).rejects.toThrow('manifest');
});
it('temporarily applies package context for native helpers, services and private host bindings', () => {
  vi.stubEnv('SHELLFOX_RESOURCES_PATH', '/old/resources'); vi.stubEnv('SHELLFOX_RUNTIME_PATH', '/old/runtime');
  const restore = applyRuntimeContext({ resourcesPath: '/desktop/resources', headless: false });
  expect(process.env.SHELLFOX_RESOURCES_PATH).toBe('/desktop/resources'); expect(process.env.SHELLFOX_RUNTIME_PATH).toBeUndefined();
  restore(); expect(process.env.SHELLFOX_RESOURCES_PATH).toBe('/old/resources'); expect(process.env.SHELLFOX_RUNTIME_PATH).toBe('/old/runtime');
});
