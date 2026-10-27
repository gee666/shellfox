import { createHash } from 'node:crypto';
import { access, chmod, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { afterEach, expect, it, vi } from 'vitest';
import { LATEST_RELEASE_URL } from '../main/update-check';
import { parseUpdateArgs, runHeadlessUpdate } from './update';
import { requiredHeadlessExecutables, requiredHeadlessFiles } from './headless-installation';
import { UPDATE_POINTER } from './update-activation';
import type { UpdateFetch } from '../main/update/self-updater';

const cleanup: string[] = [];
afterEach(async () => { vi.unstubAllEnvs(); for (const root of cleanup.splice(0)) await rm(root, { recursive: true, force: true }); });
const content = Buffer.from('mock release archive, never executed');
const digest = 'sha256:' + createHash('sha256').update(content).digest('hex');
const targets = ['win32', 'darwin', 'linux'].flatMap(platform => ['x64', 'arm64'].map(arch => ({ platform, arch })));
async function packageFixture(root: string, platform: string, arch: string, version = '0.9.5') {
  await mkdir(path.join(root, 'app'), { recursive: true });
  await mkdir(path.join(root, 'bin'), { recursive: true });
  await mkdir(path.join(root, 'resources'), { recursive: true });
  await mkdir(path.join(root, 'service'), { recursive: true });
  const node = platform === 'win32' ? 'bin/node.exe' : 'bin/node';
  await writeFile(path.join(root, 'runtime.json'), JSON.stringify({ version: 1, node, entry: 'app/main.cjs', resources: 'resources', runAsNode: false, headless: true }));
  await writeFile(path.join(root, 'build.json'), JSON.stringify({ product: 'headless', version, platform, arch, sourceHash: 'fixture' }));
  for (const name of requiredHeadlessFiles(platform)) {
    await mkdir(path.dirname(path.join(root, name)), { recursive: true });
    await writeFile(path.join(root, name), 'test fixture; never execute');
  }
  for (const name of requiredHeadlessExecutables(platform)) await chmod(path.join(root, name), 0o755);
}
async function fixture(platform = 'linux', arch = 'x64') {
  await mkdir('tmp', { recursive: true });
  const root = await mkdtemp(path.resolve('tmp/headless update ')); cleanup.push(root);
  const current = path.join(root, 'current');
  await packageFixture(current, platform, arch);
  const name = `shellfox-headless-0.9.6-${platform}-${arch}.${platform === 'win32' ? 'zip' : 'tar.gz'}`;
  const release = { tag_name: 'v0.9.6', draft: false, prerelease: false, assets: [{ name, size: content.length, digest, browser_download_url: `https://github.com/gee666/shellfox/releases/download/v0.9.6/${name}` }] };
  const fetch = vi.fn<UpdateFetch>(async url => url === LATEST_RELEASE_URL ? Response.json(release) : new Response(content));
  const extract = vi.fn(async (_archive: string, destination: string, name: string) => {
    const installation = path.join(destination, name);
    await packageFixture(installation, platform, arch, '0.9.6'); return installation;
  });
  const options = { current: '0.9.5', entry: path.join(current, 'app/main.cjs'), platform, arch, fetch, extract, check: false };
  return { root, current, release, fetch, extract, options };
}
it.each(targets)('selects only the $platform/$arch headless asset and stages beside the current copy', async ({ platform, arch }) => {
  const f = await fixture(platform, arch);
  // Inherited desktop launcher and runtime hints must not change package selection.
  vi.stubEnv('SHELLFOX_RUNTIME_PATH', 'not-the-installed-package');
  vi.stubEnv('SHELLFOX_CLI_LAUNCH', 'not-a-desktop-launch');
  const before = await readFile(path.join(f.current, 'build.json'), 'utf8');
  const result = await runHeadlessUpdate(f.options);
  expect(result).toMatchObject({ current: '0.9.5', latest: '0.9.6', available: true, phase: 'installed', restartRequired: true });
  expect(result.installation).toContain(f.root);
  expect(result.message).toContain('active host and its terminals are unchanged');
  expect(JSON.parse(await readFile(path.join(f.current, UPDATE_POINTER), 'utf8'))).toEqual({ version: 1, installation: result.installation });
  expect(result.message).toContain('--user-data');
  expect(await readFile(path.join(f.current, 'build.json'), 'utf8')).toBe(before);
  expect(f.fetch).toHaveBeenCalledTimes(2);
  expect(f.fetch.mock.calls[1][0]).toBe(f.release.assets[0].browser_download_url);
  expect(f.extract).toHaveBeenCalledWith(expect.any(String), expect.any(String), `shellfox-headless-${platform}-${arch}`, platform);
  const stage = (await readdir(f.root)).find(name => name.startsWith('shellfox-headless-0.9.6-'));
  if (!stage) throw new Error('Missing staged installation.');
  expect(await readdir(path.join(f.root, stage))).toEqual(['installation']);
});
it('checks without writing staging files, downloading, extracting, or contacting a host', async () => {
  const f = await fixture();
  const result = await runHeadlessUpdate({ ...f.options, check: true });
  expect(result).toMatchObject({ phase: 'available', installation: null, restartRequired: false });
  expect(f.fetch).toHaveBeenCalledTimes(1); expect(f.extract).not.toHaveBeenCalled();
  expect(await readdir(f.root)).toEqual(['current']);
});
it.each(['v0.9.5', 'v0.9.4'])('does not downgrade or reinstall %s', async tag => {
  const f = await fixture(); f.release.tag_name = tag;
  const result = await runHeadlessUpdate(f.options);
  expect(result).toMatchObject({ phase: 'current', available: false, restartRequired: false });
  expect(f.fetch).toHaveBeenCalledTimes(1); expect(f.extract).not.toHaveBeenCalled();
});
it('reports no published release without claiming a network failure', async () => {
  const f = await fixture(); f.fetch.mockResolvedValue(new Response('', { status: 404 }));
  expect(await runHeadlessUpdate(f.options)).toMatchObject({ latest: null, available: false, phase: 'current' });
});
it.each(['offline', 'rate-limit', 'malformed', 'draft', 'prerelease', 'html', 'large'])('fails closed on %s metadata', async kind => {
  const f = await fixture();
  if (kind === 'offline') f.fetch.mockRejectedValue(new Error('Offline'));
  if (kind === 'rate-limit') f.fetch.mockResolvedValue(new Response('', { status: 403 }));
  if (kind === 'malformed') f.fetch.mockResolvedValue(Response.json({ tag_name: '../bad' }));
  if (kind === 'draft') f.release.draft = true;
  if (kind === 'prerelease') f.release.prerelease = true;
  if (kind === 'html') f.fetch.mockResolvedValue(new Response('<html>'));
  if (kind === 'large') f.fetch.mockResolvedValue(new Response(' '.repeat(2 * 1024 * 1024 + 1)));
  await expect(runHeadlessUpdate(f.options)).rejects.toMatchObject({ code: 'UPDATE_FAILED' });
  expect(f.extract).not.toHaveBeenCalled(); expect(await readdir(f.root)).toEqual(['current']);
});
it.each(['missing', 'duplicate', 'digest', 'origin', 'size', 'desktop-only'])('rejects %s headless assets before staging', async kind => {
  const f = await fixture();
  if (kind === 'missing') f.release.assets = [];
  if (kind === 'duplicate') f.release.assets.push({ ...f.release.assets[0] });
  if (kind === 'digest') f.release.assets[0].digest = '';
  if (kind === 'origin') f.release.assets[0].browser_download_url = 'https://example.com/archive';
  if (kind === 'size') f.release.assets[0].size = 0;
  if (kind === 'desktop-only') f.release.assets[0].name = 'ShellfoxSetup.exe';
  await expect(runHeadlessUpdate({ ...f.options, check: true })).rejects.toMatchObject({ code: 'UNSUPPORTED' });
  expect(f.fetch).toHaveBeenCalledTimes(1); expect(f.extract).not.toHaveBeenCalled();
  expect(await readdir(f.root)).toEqual(['current']);
});
it.each(['checksum', 'size', 'extract', 'identity', 'runtime', 'native-addon', 'native-helper'])('removes failed %s staging and keeps the current package intact', async kind => {
  const f = await fixture();
  if (kind === 'checksum') f.fetch.mockImplementation(async url => url === LATEST_RELEASE_URL ? Response.json(f.release) : new Response(Buffer.alloc(content.length)));
  if (kind === 'size') f.fetch.mockImplementation(async url => url === LATEST_RELEASE_URL ? Response.json(f.release) : new Response('short'));
  if (kind === 'extract') f.extract.mockRejectedValue(new Error('tar unavailable'));
  if (['identity', 'runtime', 'native-addon', 'native-helper'].includes(kind)) f.extract.mockImplementation(async (_archive, destination, name) => {
    const installation = path.join(destination, name);
    await packageFixture(installation, 'linux', 'x64', kind === 'identity' ? '0.9.7' : '0.9.6');
    if (kind === 'runtime') await writeFile(path.join(installation, 'runtime.json'), '{}');
    if (kind === 'native-addon') await rm(path.join(installation, 'app/node_modules/node-pty/build/Release/pty.node'));
    if (kind === 'native-helper') await rm(path.join(installation, 'resources/tailscale/shellfox-tailscale'));
    return installation;
  });
  await expect(runHeadlessUpdate(f.options)).rejects.toMatchObject({ code: 'UPDATE_FAILED' });
  expect(await readdir(f.root)).toEqual(['current']);
  await expect(access(path.join(f.current, 'app/main.cjs'))).resolves.toBeUndefined();
});
it('keeps the active pointer when a later verified package is incomplete', async () => {
  const f = await fixture();
  const first = await runHeadlessUpdate(f.options);
  if (!first.installation) throw new Error('Missing installed version.');
  const pointer = await readFile(path.join(f.current, UPDATE_POINTER), 'utf8');
  f.release.tag_name = 'v0.9.7';
  f.release.assets[0].name = f.release.assets[0].name.replace('0.9.6', '0.9.7');
  f.release.assets[0].browser_download_url = f.release.assets[0].browser_download_url.replaceAll('0.9.6', '0.9.7');
  // The archive passes size and digest checks, but extraction gives the wrong package identity.
  await expect(runHeadlessUpdate({ ...f.options, entry: path.join(first.installation, 'app/main.cjs'), current: '0.9.6' })).rejects.toMatchObject({ code: 'UPDATE_FAILED' });
  expect(await readFile(path.join(f.current, UPDATE_POINTER), 'utf8')).toBe(pointer);
  expect((await readdir(f.root)).filter(name => name.startsWith('shellfox-headless-'))).toHaveLength(1);
});
it('refuses unsupported targets and source checkouts before network or filesystem changes', async () => {
  const f = await fixture();
  await expect(runHeadlessUpdate({ ...f.options, platform: 'freebsd' })).rejects.toMatchObject({ code: 'UNSUPPORTED' });
  await expect(runHeadlessUpdate({ ...f.options, arch: 'ia32' })).rejects.toMatchObject({ code: 'UNSUPPORTED' });
  await expect(runHeadlessUpdate({ ...f.options, entry: path.join(f.root, 'source/main.cjs') })).rejects.toMatchObject({ code: 'UNSUPPORTED' });
  expect(f.fetch).not.toHaveBeenCalled();
});
it.each([['--check', '--check'], ['--force'], ['extra'], ['--check=yes'], ['--check', 'extra']])('rejects invalid update arguments %j', (...args) => {
  expect(() => parseUpdateArgs(args)).toThrow('Usage: shellfox update');
});
it('accepts check and local selection, but never forwards update to a remote host', () => {
  expect(parseUpdateArgs([])).toEqual({ check: false });
  expect(parseUpdateArgs(['--check'], 'local')).toEqual({ check: true });
  expect(() => parseUpdateArgs([], 'peer-id')).toThrow('local installation');
});
