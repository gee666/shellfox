import path from 'node:path';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
const mocks = vi.hoisted(() => ({ stable: vi.fn(), legacy: vi.fn() }));
vi.mock('./stable-helper', () => ({ prepareWindowsHelper: mocks.stable, pinnedLegacyHelper: mocks.legacy }));
import { hostNetworkHelper, hostResourcePaths } from './paths';

const anchor = 'a'.repeat(64);
function platform(value: string) {
  vi.stubGlobal('process', new Proxy(process, { get(target, key) {
    if (key === 'platform') return value;
    if (key === 'arch') return 'x64';
    return Reflect.get(target, key);
  } }));
}
beforeEach(() => {
  mocks.stable.mockReset().mockResolvedValue({ executable: 'stable', args: ['--expected-sha256', anchor] });
  mocks.legacy.mockReset().mockResolvedValue({ executable: 'legacy', args: [] });
  vi.stubGlobal('__TAILSCALE_HELPER_SHA256__', anchor);
  vi.stubEnv('LOCALAPPDATA', path.resolve('tmp/network-helper-unit-local'));
});
afterEach(() => { vi.unstubAllGlobals(); vi.unstubAllEnvs(); });

it('routes both packaged and development Windows candidates through the compiled-pin stable policy', async () => {
  platform('win32');
  const options = { packaged: true, resourcesPath: path.resolve('tmp/app-A/resources') };
  expect(await hostNetworkHelper(options)).toEqual({ executable: 'stable', args: ['--expected-sha256', anchor] });
  expect(mocks.stable).toHaveBeenLastCalledWith({ candidate: hostResourcePaths(options).tailscale, expectedSha256: anchor, localAppData: process.env.LOCALAPPDATA, arch: 'x64' });
  const development = { packaged: false, projectRoot: path.resolve('tmp/development') };
  await hostNetworkHelper(development);
  expect(mocks.stable).toHaveBeenLastCalledWith(expect.objectContaining({ candidate: hostResourcePaths(development).tailscale, expectedSha256: anchor }));
  expect(mocks.legacy).not.toHaveBeenCalled();
});

it('preserves explicit old helper CLI/path with its supplied pin rather than silently restaging it', async () => {
  platform('win32');
  const helperPath = path.resolve('tmp/legacy-fixture/shellfox-tailscale.exe');
  await hostNetworkHelper({ helperPath, helperSha256: 'b'.repeat(64) });
  expect(mocks.legacy).toHaveBeenCalledExactlyOnceWith(helperPath, 'b'.repeat(64));
  expect(mocks.stable).not.toHaveBeenCalled();
});

it('does not infer trust from a Windows candidate when a build anchor is absent', async () => {
  platform('win32'); vi.stubGlobal('__TAILSCALE_HELPER_SHA256__', undefined);
  await expect(hostNetworkHelper({ packaged: true, resourcesPath: path.resolve('tmp/app-A/resources') })).rejects.toThrow('no trusted build SHA256');
  expect(mocks.stable).not.toHaveBeenCalled();
  expect(mocks.legacy).not.toHaveBeenCalled();
});

it('propagates a stable update failure without trying legacy or versioned networking', async () => {
  platform('win32'); mocks.stable.mockRejectedValue(new Error('Busy target'));
  await expect(hostNetworkHelper()).rejects.toThrow('Busy target');
  expect(mocks.legacy).not.toHaveBeenCalled();
});

it.each(['darwin', 'linux'])('keeps %s network helpers in their signed bundle/package layout', async value => {
  platform(value);
  const resourcesPath = path.resolve('tmp/app-A/resources');
  expect(await hostNetworkHelper({ resourcesPath, packaged: true })).toEqual({ executable: path.join(resourcesPath, 'tailscale/shellfox-tailscale'), args: [] });
  expect(mocks.stable).not.toHaveBeenCalled(); expect(mocks.legacy).not.toHaveBeenCalled();
});
