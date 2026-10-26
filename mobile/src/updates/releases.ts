import { z } from 'zod';

export const RELEASE_REPOSITORY = 'gee666/shellfox';
export const LATEST_RELEASE_URL = `https://api.github.com/repos/${RELEASE_REPOSITORY}/releases/latest`;
export const MAX_APK_BYTES = 512 * 1024 * 1024;
const stableVersion = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const assetSchema = z.object({
  name: z.string(), browser_download_url: z.string(), size: z.number().int().positive().max(MAX_APK_BYTES),
  digest: z.string().regex(/^sha256:[a-fA-F0-9]{64}$/),
});
const releaseSchema = z.object({
  tag_name: z.string(), draft: z.literal(false), prerelease: z.literal(false), assets: z.array(z.unknown()).max(1000),
});

export interface AndroidUpdate { version: string; url: string; sha256: string; size: number }

function components(version: string): number[] | null {
  if (!stableVersion.test(version)) return null;
  const parts = version.split('.').map(Number);
  return parts.every(Number.isSafeInteger) ? parts : null;
}

export function isNewerVersion(candidate: string, current: string): boolean {
  const next = components(candidate), installed = components(current);
  if (!next || !installed) return false;
  for (let i = 0; i < 3; i++) {
    if (next[i] !== installed[i]) return next[i]! > installed[i]!;
  }
  return false;
}

export function parseAndroidRelease(body: unknown, current: string): AndroidUpdate | null {
  const parsed = releaseSchema.safeParse(body);
  if (!parsed.success) return null;
  const release = parsed.data;
  if (!release.tag_name.startsWith('v')) return null;
  const version = release.tag_name.slice(1);
  if (!isNewerVersion(version, current)) return null;
  const name = `shellfox-mobile-${version}-android.apk`;
  const matches = release.assets.filter(asset =>
    typeof asset === 'object' && asset !== null && 'name' in asset && asset.name === name);
  if (matches.length !== 1) return null;
  const asset = assetSchema.safeParse(matches[0]);
  if (!asset.success) return null;
  // Exact comparison also rejects credentials, ports, queries, fragments and encoded path tricks.
  const expected = `https://github.com/${RELEASE_REPOSITORY}/releases/download/v${version}/${name}`;
  if (asset.data.browser_download_url !== expected) return null;
  return { version, url: expected, sha256: asset.data.digest.slice(7).toLowerCase(), size: asset.data.size };
}
