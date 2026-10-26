import path from 'node:path';
import { afterEach, expect, it, vi } from 'vitest';
import { hostResourcePaths } from './paths';

afterEach(() => { vi.unstubAllGlobals(); vi.unstubAllEnvs(); });
function runtime(resourcesPath: string | undefined, defaultApp: boolean) {
  vi.stubGlobal('process', new Proxy(process, { get(target, key) {
    if (key === 'resourcesPath') return resourcesPath;
    if (key === 'defaultApp') return defaultApp;
    if (key === 'versions') return { ...target.versions, electron: 'fixture' };
    return Reflect.get(target, key);
  } }));
}

it('resolves a packaged Electron helper without importing Electron or host services', () => {
  vi.stubEnv('SHELLFOX_RESOURCES_PATH', undefined);
  const resources = path.resolve('tmp/electron/resources');
  runtime(resources, false);
  expect(hostResourcePaths().tailscale).toBe(path.join(resources, 'tailscale', 'shellfox-tailscale' + (process.platform === 'win32' ? '.exe' : '')));
  expect(hostResourcePaths().helpers.packaged).toBe(true);
});

it('ignores the development Electron runtime resources directory', () => {
  vi.stubEnv('SHELLFOX_RESOURCES_PATH', undefined);
  vi.stubEnv('SHELLFOX_PROJECT_ROOT', path.resolve('tmp/project'));
  runtime(path.resolve('tmp/electron/resources'), true);
  const paths = hostResourcePaths();
  expect(paths.helpers.packaged).toBe(false);
  expect(paths.tailscale).toContain(path.join('tmp/project', 'tmp/tailscale', process.platform + '-' + process.arch));
});

it('pins packaged Electron resources against foreign inherited environment but honors explicit options', () => {
  runtime(path.resolve('tmp/electron/resources'), false);
  vi.stubEnv('SHELLFOX_RESOURCES_PATH', path.resolve('tmp/headless/resources'));
  expect(hostResourcePaths().tailscale).toContain(path.join('electron', 'resources', 'tailscale'));
  expect(hostResourcePaths({ resourcesPath: path.resolve('tmp/explicit/resources') }).tailscale).toContain(path.join('explicit', 'resources', 'tailscale'));
});

it('resolves launcher-provided headless resources without Electron globals', () => {
  vi.stubEnv('SHELLFOX_RESOURCES_PATH', path.resolve('tmp/headless/resources'));
  expect(hostResourcePaths().tailscale).toContain(path.join('headless', 'resources', 'tailscale'));
});
