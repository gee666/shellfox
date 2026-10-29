import { describe, expect, it, vi } from 'vitest';
import { createUpdateClient } from './client';
import { LATEST_RELEASE_URL } from './releases';

const url = 'https://github.com/gee666/shellfox/releases/download/v0.9.0/shellfox-mobile-0.9.0-android.apk';
const release = {
  tag_name: 'v0.9.0', draft: false, prerelease: false,
  assets: [{ name: 'shellfox-mobile-0.9.0-android.apk', size: 123, browser_download_url: url, digest: `sha256:${'ab'.repeat(32)}` }],
};
function fixture(platform = 'android') {
  const bridge = { getVersion: vi.fn(async () => '0.8.0'), download: vi.fn(async () => {}), install: vi.fn(async () => {}) };
  const fetch = vi.fn<typeof globalThis.fetch>(async () => new Response(JSON.stringify(release)));
  return { bridge, fetch, client: createUpdateClient({ platform, bridge, fetch, timeoutMs: 10 }) };
}

describe('mobile update client', () => {
  it('downloads and verifies without opening the installer, then reuses the staged version', async () => {
    const { client, bridge } = fixture();
    expect(await client.download()).toMatchObject({ version: '0.9.0' });
    await client.download();
    expect(bridge.download).toHaveBeenCalledExactlyOnceWith(url, 'ab'.repeat(32));
    expect(bridge.install).not.toHaveBeenCalled();
    expect(await client.install()).toBe(true);
    expect(bridge.install).toHaveBeenCalledExactlyOnceWith(url, 'ab'.repeat(32));
  });
  it('checks the setting again after an in-flight lookup', async () => {
    const { client, bridge } = fixture();
    expect(await client.download(() => false)).toBeNull();
    expect(bridge.download).not.toHaveBeenCalled(); expect(bridge.install).not.toHaveBeenCalled();
  });
  it('deduplicates downloads and waits before a manual install', async () => {
    const { client, bridge } = fixture();
    let complete!: () => void;
    bridge.download.mockImplementationOnce(() => new Promise<void>(resolve => { complete = resolve; }));
    const first = client.download(), second = client.download();
    await vi.waitFor(() => expect(bridge.download).toHaveBeenCalledTimes(1));
    const installing = client.install();
    expect(bridge.install).not.toHaveBeenCalled();
    complete();
    await Promise.all([first, second, installing]);
    expect(bridge.install).toHaveBeenCalledTimes(1);
  });
  it('can retry a failed automatic download manually', async () => {
    const { client, bridge } = fixture();
    bridge.download.mockRejectedValueOnce(new Error('offline'));
    await expect(client.download()).rejects.toThrow('offline');
    expect(await client.install()).toBe(true);
  });
  it('checks the fixed metadata endpoint without following redirects', async () => {
    const { client, fetch, bridge } = fixture();
    expect(await client.check()).toMatchObject({ version: '0.9.0' });
    expect(fetch).toHaveBeenCalledWith(LATEST_RELEASE_URL, expect.objectContaining({ redirect: 'error', signal: expect.any(AbortSignal) }));
    expect(bridge.install).not.toHaveBeenCalled();
  });
  it('rechecks at installation and passes only verified URL and checksum', async () => {
    const { client, bridge, fetch } = fixture();
    await client.check();
    expect(await client.install()).toBe(true);
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(bridge.install).toHaveBeenCalledWith(url, 'ab'.repeat(32));
  });
  it('does not install stale metadata after an offline check', async () => {
    const { client, bridge, fetch } = fixture();
    await client.check();
    fetch.mockRejectedValueOnce(new Error('offline'));
    await expect(client.install()).rejects.toThrow('offline');
    expect(bridge.install).not.toHaveBeenCalled();
  });
  it('does not install absent, older or unverified releases', async () => {
    const { client, bridge, fetch } = fixture();
    fetch.mockResolvedValueOnce(new Response(JSON.stringify({ ...release, assets: [] })));
    expect(await client.install()).toBe(false);
    expect(bridge.install).not.toHaveBeenCalled();
  });
  it.each(['ios', 'windows', 'macos', 'web'])('does not attempt APK updates on %s', async platform => {
    const { client, bridge, fetch } = fixture(platform);
    expect(await client.check()).toBeNull();
    await expect(client.install()).rejects.toThrow('only supported on Android');
    expect(fetch).not.toHaveBeenCalled();
    expect(bridge.getVersion).not.toHaveBeenCalled();
  });
  it('propagates installer/permission failures and allows retry', async () => {
    const { client, bridge } = fixture();
    bridge.install.mockRejectedValueOnce(new Error('permission denied'));
    await expect(client.install()).rejects.toThrow('permission denied');
    expect(await client.install()).toBe(true);
  });
  it('rejects concurrent installations', async () => {
    const { client, bridge } = fixture();
    let complete: (() => void) | undefined;
    bridge.install.mockImplementationOnce(() => new Promise<void>(resolve => { complete = resolve; }));
    const first = client.install();
    await vi.waitFor(() => expect(bridge.install).toHaveBeenCalled());
    await expect(client.install()).rejects.toThrow('already in progress');
    complete?.();
    expect(await first).toBe(true);
  });
  it('rejects HTTP failures and malformed/oversized metadata', async () => {
    const { client, fetch } = fixture();
    fetch.mockResolvedValueOnce(new Response('', { status: 503 }));
    await expect(client.check()).rejects.toThrow('Could not check');
    fetch.mockResolvedValueOnce(new Response('{bad'));
    await expect(client.check()).rejects.toThrow();
    fetch.mockResolvedValueOnce(new Response('{}', { headers: { 'content-length': '2097153' } }));
    await expect(client.check()).rejects.toThrow('too large');
    fetch.mockResolvedValueOnce(new Response(' '.repeat(2097153)));
    await expect(client.check()).rejects.toThrow('too large');
  });
  it('aborts timed-out metadata requests', async () => {
    const { client, fetch } = fixture();
    fetch.mockImplementationOnce(async (_input, init) => new Promise((_resolve, reject) => {
      init?.signal?.addEventListener('abort', () => reject(new Error('aborted')), { once: true });
    }));
    await expect(client.check()).rejects.toThrow('aborted');
  });
});
