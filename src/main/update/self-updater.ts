import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { mkdtemp, open, rm } from 'node:fs/promises';
import path from 'node:path';
import { failure, success, type Result, type UpdateStatusDto } from '../../shared/contracts';
import { isNewer, LATEST_RELEASE_URL, parseVersion, RELEASES_URL, UPDATE_COMMAND } from '../update-check';
import type { InstallHandoff } from './handoff';

export const CHECK_INTERVAL_MS = 60 * 60 * 1000;
export const MAX_DOWNLOAD_BYTES = 768 * 1024 * 1024;
export interface ReleaseAsset { name: string; url: string; size: number; sha256: string }
export interface UpdateRelease { version: string; asset: ReleaseAsset | null }
export interface UpdatePlatform {
  supported: boolean; reason: string | null;
  assetName(version: string): string;
  prepare(file: string, version: string, work: string): Promise<() => Promise<InstallHandoff>>;
  cleanup?(): Promise<void>;
}
export interface UpdateSource {
  status(): Promise<UpdateStatusDto>;
  download?(): Promise<Result<UpdateStatusDto>>;
  install?(): Promise<Result<UpdateStatusDto>>;
}
export type QuitInstallResult = 'not-ready' | 'cancelled' | 'installed';
export type UpdateFetch = (url: string, init: RequestInit) => Promise<Response>;

async function readReleaseBody(response: Response): Promise<unknown> {
  if (!response.body) throw new Error('Missing release metadata.');
  const reader = response.body.getReader(), chunks: Uint8Array[] = [];
  let size = 0;
  try {
    for (;;) {
      const chunk = await reader.read();
      if (chunk.done) break;
      size += chunk.value.byteLength;
      if (size > 2 * 1024 * 1024) throw new Error('Release metadata is too large.');
      chunks.push(chunk.value);
    }
    return JSON.parse(Buffer.concat(chunks).toString('utf8'));
  } finally { await reader.cancel().catch(() => {}); }
}

// Never accept renderer-supplied URLs, paths, commands or environment overrides.
export function parseRelease(body: unknown, assetName: (version: string) => string): UpdateRelease | null {
  if (!body || typeof body !== 'object') return null;
  const release = body as Record<string, unknown>;
  const version = parseVersion(release.tag_name);
  if (!version || release.draft === true || release.prerelease === true || !/^\d+\.\d+\.\d+$/.test(version)) return null;
  const expected = assetName(version);
  const assets = Array.isArray(release.assets) ? release.assets : [];
  const matches = assets.filter(a => a && typeof a === 'object' && a.name === expected);
  let asset: ReleaseAsset | null = null;
  if (matches.length === 1) {
    const a = matches[0];
    const digest = typeof a.digest === 'string' ? /^sha256:([a-fA-F0-9]{64})$/.exec(a.digest) : null;
    try {
      const url = new URL(a.browser_download_url);
      const expectedPath = `/gee666/shellfox/releases/download/${encodeURIComponent(String(release.tag_name))}/${expected}`;
      if (url.protocol === 'https:' && url.hostname === 'github.com' && !url.port && !url.username && !url.password && !url.search && !url.hash && url.pathname === expectedPath && digest && Number.isSafeInteger(a.size) && a.size > 0 && a.size <= MAX_DOWNLOAD_BYTES) {
        asset = { name: expected, url: url.href, size: a.size, sha256: digest[1]!.toLowerCase() };
      }
    } catch { /* Missing or untrusted asset: show the version, but do not install. */ }
  }
  return { version, asset };
}

export async function hashFile(file: string): Promise<string> {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest('hex');
}

export async function downloadAsset(fetch: UpdateFetch, asset: ReleaseAsset, destination: string, progress: (received: number) => void, signal: AbortSignal): Promise<void> {
  const response = await fetch(asset.url, { signal, headers: { 'User-Agent': 'shellfox-updater' }, redirect: 'follow' });
  // GitHub redirects release downloads to its asset CDN. No other final origin is allowed.
  const final = new URL(response.url || asset.url);
  if (final.protocol !== 'https:' || !['github.com', 'release-assets.githubusercontent.com', 'objects.githubusercontent.com'].includes(final.hostname) || final.username || final.password || final.port) throw new Error('Untrusted download origin.');
  if (!response.ok || !response.body) throw new Error('Release download failed.');
  const file = await open(destination, 'wx', 0o600);
  const reader = response.body.getReader();
  let received = 0;
  const hash = createHash('sha256');
  try {
    for (;;) {
      const chunk = await reader.read();
      if (chunk.done) break;
      received += chunk.value.byteLength;
      if (received > asset.size || received > MAX_DOWNLOAD_BYTES) throw new Error('Download exceeds the release size.');
      hash.update(chunk.value);
      // FileHandle.write may write fewer bytes than requested.
      let offset = 0;
      while (offset < chunk.value.byteLength) {
        const written = await file.write(chunk.value, offset, chunk.value.byteLength - offset);
        if (!written.bytesWritten) throw new Error('Could not save download.');
        offset += written.bytesWritten;
      }
      progress(received);
    }
    if (received !== asset.size || hash.digest('hex') !== asset.sha256) throw new Error('Release checksum verification failed.');
    await file.sync();
  } finally { await reader.cancel().catch(() => {}); await file.close(); }
}

export class SelfUpdater implements UpdateSource {
  private release: UpdateRelease | null = null;
  private state: UpdateStatusDto;
  private lookup: Promise<UpdateStatusDto> | null = null;
  private checkedAt = -Infinity;
  private operation: Promise<void> | null = null;
  private timer: ReturnType<typeof setInterval> | null = null;
  private controller: AbortController | null = null;
  private work: string | null = null;
  private disposed = false;
  private launch: (() => Promise<InstallHandoff>) | null = null;
  private starting: Promise<InstallHandoff> | null = null;
  private activeHandoff: InstallHandoff | null = null;
  private handedOff = false;
  private installing = false;
  constructor(private readonly options: {
    current: string; platform: UpdatePlatform; fetch: UpdateFetch; tempRoot: string;
    // Confirm, start/acknowledge the helper, reversibly close terminals, then commit.
    quitForUpdate: (start: () => Promise<InstallHandoff>, onQuit: boolean) => Promise<boolean>;
    now?: () => number;
  }) {
    this.state = { current: options.current, latest: null, available: false, command: UPDATE_COMMAND, url: RELEASES_URL,
      phase: 'idle', supported: options.platform.supported, reason: options.platform.reason, received: 0, total: null, error: null };
  }
  start(): void {
    if (this.timer || this.disposed) return;
    void this.status();
    this.timer = setInterval(() => { void this.status(); }, CHECK_INTERVAL_MS);
    this.timer.unref?.();
  }
  async status(): Promise<UpdateStatusDto> {
    if (this.disposed || this.operation || this.installing || this.state.phase === 'ready' || this.state.phase === 'installing') return { ...this.state };
    if (this.lookup) return this.lookup;
    const now = (this.options.now ?? Date.now)();
    if (now - this.checkedAt < CHECK_INTERVAL_MS) return { ...this.state };
    this.lookup ??= this.check().finally(() => { this.lookup = null; });
    return this.lookup;
  }
  private async check(): Promise<UpdateStatusDto> {
    this.checkedAt = (this.options.now ?? Date.now)();
    const controller = new AbortController(); this.controller = controller;
    const timer = setTimeout(() => controller.abort(), 10_000);
    try {
      const response = await this.options.fetch(LATEST_RELEASE_URL, { signal: controller.signal, redirect: 'error', headers: { Accept: 'application/vnd.github+json', 'User-Agent': 'shellfox-updater' } });
      if (!response.ok) throw new Error('Release check failed.');
      const release = parseRelease(await readReleaseBody(response), v => this.options.platform.assetName(v));
      if (this.disposed) return { ...this.state };
      this.release = release;
      this.state.latest = release?.version ?? null;
      this.state.available = !!release && isNewer(release.version, this.options.current);
      this.state.supported = this.options.platform.supported && !!release?.asset;
      this.state.reason = this.options.platform.reason ?? (release && !release.asset ? 'This release has no verified installer for this platform. Download it from the releases page.' : null);
    } catch { /* Offline checks keep the last known release. */ }
    finally { clearTimeout(timer); if (this.controller === controller) this.controller = null; }
    return { ...this.state };
  }
  async download(): Promise<Result<UpdateStatusDto>> {
    if (this.disposed) return failure('UNSUPPORTED', 'Updater is shutting down.');
    await this.status();
    if (this.disposed) return failure('UNSUPPORTED', 'Updater is shutting down.');
    if (this.operation || this.installing || this.state.phase === 'ready' || this.state.phase === 'installing') return success({ ...this.state });
    const asset = this.release?.asset;
    if (!this.state.available || !this.state.supported || !asset) return failure('UNSUPPORTED', this.state.reason ?? 'No installable update is available.');
    this.state = { ...this.state, phase: 'downloading', received: 0, total: asset.size, error: null };
    // Start in the background. IPC returns immediately; status polling reports progress.
    this.operation = this.performDownload(asset, this.release!.version).catch(() => {
      this.state.phase = 'error';
      this.state.error = 'Failed update files could not be removed. Retry the download or quit Shellfox to clean them up.';
    }).finally(() => { this.operation = null; });
    return success({ ...this.state });
  }
  private async performDownload(asset: ReleaseAsset, version: string): Promise<void> {
    const controller = new AbortController(); this.controller = controller;
    const timer = setTimeout(() => controller.abort(), 30 * 60 * 1000);
    try {
      await this.cleanWork();
      this.work = await mkdtemp(path.join(this.options.tempRoot, 'shellfox-update-'));
      const file = path.join(this.work, asset.name);
      await downloadAsset(this.options.fetch, asset, file, received => { this.state.received = received; }, controller.signal);
      if (this.disposed) throw new Error('Updater stopped.');
      const prepared = await this.options.platform.prepare(file, version, this.work);
      // Verification and helper startup happen before any terminal is closed.
      this.launch = async () => {
        try { if (await hashFile(file) !== asset.sha256) throw new Error('Checksum changed.'); }
        catch {
          this.state.phase = 'error'; this.state.error = 'Downloaded installer changed or is missing. Download the update again.';
          throw new Error('Downloaded installer changed.');
        }
        if (this.disposed) throw new Error('Updater stopped.');
        const starting = prepared(); this.starting = starting;
        try {
          const handoff = await starting;
          this.activeHandoff = handoff;
          if (this.disposed) { await handoff.cancel(); throw new Error('Updater stopped.'); }
          return {
            get committed() { return handoff.committed; },
            commit: async () => {
              if (this.disposed) throw new Error('Updater stopped.');
              await handoff.commit(); this.handedOff = true;
            },
            cancel: async () => { await handoff.cancel(); if (this.activeHandoff === handoff) this.activeHandoff = null; },
          };
        } finally { if (this.starting === starting) this.starting = null; }
      };
      this.state.phase = 'ready';
    } catch {
      this.state.phase = 'error'; this.state.error = 'The update could not be downloaded or verified. Try again, or download it from the releases page.';
      await this.cleanWork();
    } finally { controller.abort(); clearTimeout(timer); if (this.controller === controller) this.controller = null; }
  }
  private canInstall(): boolean {
    return !this.disposed && !this.installing && !this.handedOff && this.state.phase === 'ready' && !!this.launch;
  }
  async install(): Promise<Result<UpdateStatusDto>> {
    if (!this.canInstall()) return failure('VALIDATION', 'Download and verify the update before installing.');
    try {
      await this.installReady(false);
      return success({ ...this.state });
    } catch {
      return failure('INTERNAL', 'The update did not start. Retry the installation or update manually. Closed terminals must be reopened as fresh shells.', true);
    }
  }
  /** Inspect staged state only. Quit must not start or wait for a download. */
  async installOnQuit(): Promise<QuitInstallResult> {
    if (!this.canInstall()) return 'not-ready';
    return await this.installReady(true) ? 'installed' : 'cancelled';
  }
  private async installReady(onQuit: boolean): Promise<boolean> {
    const launch = this.launch!;
    this.installing = true;
    this.state.phase = 'installing'; this.state.error = null;
    try {
      const approved = await this.options.quitForUpdate(launch, onQuit);
      if (!approved && !this.handedOff) this.state.phase = 'ready';
      return approved;
    } catch (error) {
      this.handedOff ||= this.activeHandoff?.committed === true;
      if (!this.handedOff && this.state.phase === 'installing') this.state.phase = 'ready';
      throw error;
    } finally { this.installing = false; }
  }
  private async cleanWork(): Promise<void> {
    this.launch = null;
    const work = this.work;
    await this.options.platform.cleanup?.();
    if (work) await rm(work, { recursive: true, force: true });
    this.work = null;
  }
  async dispose(keepInstaller = false): Promise<void> {
    this.disposed = true;
    if (this.timer) clearInterval(this.timer); this.timer = null;
    this.controller?.abort();
    await this.lookup; await this.operation;
    if (!keepInstaller && !this.handedOff) {
      if (this.starting) { try { this.activeHandoff = await this.starting; } catch { /* Startup cancels its own failed helper. */ } }
      await this.activeHandoff?.cancel();
      if (this.activeHandoff?.committed) { this.handedOff = true; return; }
      this.activeHandoff = null;
      await this.cleanWork();
    }
  }
}
