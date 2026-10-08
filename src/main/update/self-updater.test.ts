import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CHECK_INTERVAL_MS, MAX_DOWNLOAD_BYTES, SelfUpdater, downloadAsset, parseRelease, type ReleaseAsset, type UpdateFetch, type UpdatePlatform } from './self-updater';
import { LATEST_RELEASE_URL } from '../update-check';
import type { InstallHandoff } from './handoff';
const content = Buffer.from('test installer');
const sha256 = createHash('sha256').update(content).digest('hex');
const asset: ReleaseAsset = { name: 'ShellfoxSetup.exe', url: 'https://github.com/gee666/shellfox/releases/download/v0.2.6/ShellfoxSetup.exe', size: content.length, sha256 };
const release = (over: Record<string, unknown> = {}) => ({ tag_name: 'v0.2.6', draft: false, prerelease: false, assets: [{ name: asset.name, browser_download_url: asset.url, size: asset.size, digest: `sha256:${sha256}`, ...over }] });
const body = (chunks: Uint8Array[] = [content], url = 'https://release-assets.githubusercontent.com/asset') => {
  const response = new Response(new ReadableStream({ start(controller) { chunks.forEach(chunk => controller.enqueue(chunk)); controller.close(); } }));
  Object.defineProperty(response, 'url', { value: url });
  return response;
};
let root: string;
beforeEach(async () => { await mkdir('tmp', { recursive: true }); root = await mkdtemp(path.resolve('tmp/in-app-update-test-')); });
afterEach(async () => { vi.useRealTimers(); await rm(root, { recursive: true, force: true }); });

describe('release trust', () => {
  it('accepts only the exact pinned repository asset with a SHA-256 digest', () => {
    expect(parseRelease(release(), () => asset.name)).toEqual({ version: '0.2.6', asset });
  });
  it.each([
    { browser_download_url: 'http://github.com/gee666/shellfox/releases/download/v0.2.6/ShellfoxSetup.exe' },
    { browser_download_url: 'https://evil.example/ShellfoxSetup.exe' },
    { browser_download_url: asset.url + '?redirect=evil' }, { browser_download_url: asset.url + '#fragment' },
    { browser_download_url: asset.url.replace('gee666', 'attacker') },
    { browser_download_url: asset.url.replace('v0.2.6', 'v0.2.5') },
    { browser_download_url: asset.url.replace('github.com', 'github.com:8443') },
    { browser_download_url: asset.url.replace('github.com', 'user@github.com') },
    { digest: undefined }, { digest: 'sha256:invalid' }, { size: 0 }, { size: MAX_DOWNLOAD_BYTES + 1 },
    { size: '12' }, { name: '../ShellfoxSetup.exe' },
  ])('does not offer an unsafe asset %j', patch => {
    expect(parseRelease(release(patch), () => asset.name)?.asset).toBeNull();
  });
  it('rejects ambiguous assets, draft, prerelease and non-release versions', () => {
    const r = release(); r.assets.push(r.assets[0]!);
    expect(parseRelease(r, () => asset.name)?.asset).toBeNull();
    for (const patch of [{ draft: true }, { prerelease: true }, { tag_name: 'v0.2.6-rc.1' }, { tag_name: '../0.2.6' }]) expect(parseRelease({ ...release(), ...patch }, () => asset.name)).toBeNull();
  });
});
describe('verified downloads', () => {
  it('streams progress and saves the exact verified file', async () => {
    const progress = vi.fn(), file = path.join(root, 'setup');
    await downloadAsset(async () => body([content.subarray(0, 5), content.subarray(5)]), asset, file, progress, new AbortController().signal);
    expect(await readFile(file)).toEqual(content); expect(progress.mock.calls).toEqual([[5], [content.length]]);
  });
  it.each(['truncated', 'oversized', 'checksum', 'untrusted redirect', 'HTTP failure'])('refuses %s downloads', async kind => {
    const response = kind === 'HTTP failure' ? new Response('error', { status: 500 }) : kind === 'untrusted redirect' ? body([content], 'https://evil.example/setup') : body([kind === 'truncated' ? content.subarray(1) : kind === 'oversized' ? Buffer.concat([content, content]) : kind === 'checksum' ? Buffer.alloc(content.length) : content]);
    await expect(downloadAsset(async () => response, asset, path.join(root, 'setup'), () => {}, new AbortController().signal)).rejects.toThrow();
  });
  it('does not overwrite an existing download', async () => {
    const file = path.join(root, 'setup'); await writeFile(file, 'keep');
    await expect(downloadAsset(async () => body(), asset, file, () => {}, new AbortController().signal)).rejects.toThrow();
    expect(await readFile(file, 'utf8')).toBe('keep');
  });
});
function fixture(over: { fetch?: UpdateFetch; platform?: Partial<UpdatePlatform>; quit?: (start: () => Promise<InstallHandoff>) => Promise<boolean> } = {}) {
  let committed = false;
  const handoff = { get committed() { return committed; }, commit: vi.fn(async () => { committed = true; }), cancel: vi.fn(async () => {}) };
  const launch = vi.fn(async () => handoff), prepare = vi.fn(async (_file: string, _version: string, _work: string) => launch), cleanup = vi.fn(async () => {});
  const platform: UpdatePlatform = { supported: true, reason: null, assetName: () => asset.name, prepare, cleanup, ...over.platform };
  const fetch = vi.fn<UpdateFetch>(over.fetch ?? (async url => url === LATEST_RELEASE_URL ? Response.json(release()) : body()));
  const quit = vi.fn(over.quit ?? (async (start: () => Promise<InstallHandoff>) => { const h = await start(); await h.commit(); return true; }));
  const updater = new SelfUpdater({ current: '0.2.5', platform, fetch, tempRoot: root, quitForUpdate: quit });
  return { updater, launch, prepare, fetch, quit, cleanup, handoff };
}
async function ready(updater: SelfUpdater) {
  await expect.poll(async () => (await updater.status()).phase).toBe('ready');
}
describe('updater lifecycle', () => {
  it('deduplicates checks and rechecks hourly while running, preserving known releases offline', async () => {
    vi.useFakeTimers(); const f = fixture();
    f.updater.start(); await f.updater.status();
    await Promise.all([f.updater.status(), f.updater.status()]); expect(f.fetch).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(CHECK_INTERVAL_MS); expect(f.fetch).toHaveBeenCalledTimes(2);
    f.fetch.mockRejectedValue(new Error('offline'));
    await vi.advanceTimersByTimeAsync(CHECK_INTERVAL_MS);
    expect(await f.updater.status()).toMatchObject({ latest: '0.2.6', available: true });
    await f.updater.dispose(); await vi.advanceTimersByTimeAsync(CHECK_INTERVAL_MS); expect(f.fetch).toHaveBeenCalledTimes(3);
  });
  it('returns immediately, verifies, stages and waits for installation confirmation', async () => {
    const f = fixture({ quit: async () => false });
    expect(await f.updater.download()).toMatchObject({ ok: true, value: { phase: 'downloading' } });
    await f.updater.download(); await ready(f.updater);
    expect(f.fetch).toHaveBeenCalledTimes(2); expect(f.prepare).toHaveBeenCalledTimes(1); expect(f.quit).not.toHaveBeenCalled();
    expect(await f.updater.install()).toMatchObject({ ok: true, value: { phase: 'ready' } });
    expect(f.launch).not.toHaveBeenCalled();
    await f.updater.dispose(); expect(await readdir(root)).toEqual([]);
  });
  it('launches exactly once on confirmed installation and ignores duplicate install requests', async () => {
    let resolve!: (value: boolean) => void;
    const f = fixture({ quit: async start => { const h = await start(); await h.commit(); return new Promise<boolean>(r => { resolve = r; }); } });
    await f.updater.download(); await ready(f.updater);
    const installing = f.updater.install();
    await expect(f.updater.install()).resolves.toMatchObject({ ok: false });
    await expect.poll(() => f.launch.mock.calls.length).toBe(1);
    resolve(true); await installing;
    await f.updater.dispose(true); expect(await readdir(root)).toHaveLength(1);
  });
  it('never installs a failed or missing download and cleans up for retry', async () => {
    const f = fixture({ fetch: async url => url === LATEST_RELEASE_URL ? Response.json(release()) : body([Buffer.alloc(content.length)]) });
    expect(await f.updater.install()).toMatchObject({ ok: false });
    await f.updater.download();
    await expect.poll(async () => (await f.updater.status()).phase).toBe('error');
    expect(f.prepare).not.toHaveBeenCalled(); expect(f.quit).not.toHaveBeenCalled(); expect(await readdir(root)).toEqual([]);
    f.fetch.mockImplementation(async url => url === LATEST_RELEASE_URL ? Response.json(release()) : body());
    await f.updater.download(); await ready(f.updater); await f.updater.dispose();
  });
  it('rechecks the checksum before handoff and does not run a changed installer', async () => {
    const f = fixture(); await f.updater.download(); await ready(f.updater);
    const file = f.prepare.mock.calls[0]![0] as string; await writeFile(file, 'tampered');
    expect(await f.updater.install()).toMatchObject({ ok: false }); expect(f.launch).not.toHaveBeenCalled();
    await f.updater.dispose();
  });
  it('refuses unsupported installations and releases without verified assets', async () => {
    for (const over of [{ platform: { supported: false, reason: 'ZIP install' } }, { fetch: async () => Response.json(release({ digest: null })) }]) {
      const f = fixture(over); expect(await f.updater.download()).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
      expect(f.prepare).not.toHaveBeenCalled(); await f.updater.dispose();
    }
  });
  it('aborts downloads on normal quit and cleans up without closing terminals itself', async () => {
    let abort!: () => void;
    const f = fixture({ fetch: async (url, init) => url === LATEST_RELEASE_URL ? Response.json(release()) : new Promise((_resolve, reject) => {
      abort = () => reject(new Error('abort')); init.signal?.addEventListener('abort', abort);
    }) });
    await f.updater.download(); await expect.poll(() => typeof abort).toBe('function');
    await f.updater.dispose(); expect(f.quit).not.toHaveBeenCalled(); expect(await readdir(root)).toEqual([]);
  });
  it('bounds metadata responses and never offers an installer for malformed release data', async () => {
    const f = fixture({ fetch: async () => new Response('x'.repeat(2 * 1024 * 1024 + 1)) });
    expect(await f.updater.status()).toMatchObject({ latest: null, available: false });
    expect(await f.updater.download()).toMatchObject({ ok: false });
    expect(f.fetch).toHaveBeenCalledTimes(1); expect(f.prepare).not.toHaveBeenCalled(); await f.updater.dispose();
  });
  it('reports cleanup failures without an unhandled background rejection and retains paths for later cleanup', async () => {
    const f = fixture();
    f.cleanup.mockResolvedValueOnce(undefined).mockRejectedValueOnce(new Error('access denied'));
    f.prepare.mockRejectedValueOnce(new Error('invalid bundle'));
    await f.updater.download();
    await expect.poll(async () => (await f.updater.status()).error).toContain('could not be removed');
    expect(await readdir(root)).toHaveLength(1);
    await f.updater.dispose(); expect(await readdir(root)).toEqual([]);
  });
  it('does not create a download after quit races a pending release lookup', async () => {
    let lookup!: (value: Response) => void;
    const f = fixture({ fetch: async () => new Promise<Response>(resolve => { lookup = resolve; }) });
    const downloading = f.updater.download(), disposed = f.updater.dispose();
    lookup(Response.json(release()));
    expect(await downloading).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
    await disposed; expect(await readdir(root)).toEqual([]); expect(f.prepare).not.toHaveBeenCalled();
  });
  it('cancels a helper whose startup completes during quit before removing its files', async () => {
    const f = fixture(); let announce!: (value: typeof f.handoff) => void;
    f.launch.mockImplementationOnce(() => new Promise(resolve => { announce = resolve; }));
    await f.updater.download(); await ready(f.updater);
    const installing = f.updater.install(); await expect.poll(() => typeof announce).toBe('function');
    const disposed = f.updater.dispose(); announce(f.handoff);
    expect(await installing).toMatchObject({ ok: false }); await disposed;
    expect(f.handoff.cancel).toHaveBeenCalled(); expect(f.handoff.commit).not.toHaveBeenCalled(); expect(await readdir(root)).toEqual([]);
  });
  it('does not delete handed-off files when normal disposal races acknowledged commit', async () => {
    let finish!: (approved: boolean) => void;
    const f = fixture({ quit: async start => { const h = await start(); await h.commit(); return new Promise(resolve => { finish = resolve; }); } });
    await f.updater.download(); await ready(f.updater); const installing = f.updater.install();
    await expect.poll(() => typeof finish).toBe('function'); await f.updater.dispose();
    expect(f.handoff.cancel).not.toHaveBeenCalled(); expect(await readdir(root)).toHaveLength(1);
    finish(true); await installing;
  });
  it('locks retries until the failed installation attempt has fully unwound', async () => {
    let releaseFailure!: () => void;
    const f = fixture({ quit: async start => {
      try { await start(); return false; }
      catch (e) { await new Promise<void>(resolve => { releaseFailure = resolve; }); throw e; }
    } });
    await f.updater.download(); await ready(f.updater);
    await writeFile(f.prepare.mock.calls[0]![0], 'tampered');
    const installing = f.updater.install(); await expect.poll(() => typeof releaseFailure).toBe('function');
    expect((await f.updater.status()).phase).toBe('error');
    await f.updater.download(); expect(f.fetch).toHaveBeenCalledTimes(2);
    releaseFailure(); await installing;
    await f.updater.download(); await ready(f.updater); expect(f.fetch).toHaveBeenCalledTimes(3); await f.updater.dispose();
  });
});
