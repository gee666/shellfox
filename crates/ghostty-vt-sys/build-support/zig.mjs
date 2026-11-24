import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { basename, join } from 'node:path';
import { fetchPinnedBytes, project, support, verifyMinisign } from './archives.mjs';

export const zigManifest = JSON.parse(readFileSync(join(support, 'zig-0.17.0.json'), 'utf8'));
// CI caches this directory. Archives in it are verified again before every use.
export const zigArchiveCache = join(project, 'tmp', 'zig-archives');

export function zigPin(platform = process.platform, arch = process.arch) {
  const host = `${{ x64: 'x86_64', arm64: 'aarch64' }[arch]}-${{ win32: 'windows', linux: 'linux', darwin: 'macos' }[platform]}`;
  const pin = zigManifest.hosts[host];
  if (!pin || !/^[a-f0-9]{64}$/.test(pin.sha256)) throw new Error(`No pinned Zig archive for ${host}`);
  return { ...pin, name: basename(new URL(pin.url).pathname) };
}

export function verifyZig(pin, data, signature, key = zigManifest.minisignKey) {
  if (createHash('sha256').update(data).digest('hex') !== pin.sha256) {
    throw new Error(`SHA-256 mismatch: ${pin.name}`);
  }
  // The trusted comment names the signed file, so a mirror can't substitute another signed archive.
  if (!verifyMinisign(data, signature, key).split('\t').includes(`file:${pin.name}`)) {
    throw new Error(`Signature is for another file than ${pin.name}`);
  }
}

// ziglang.org first, then the community mirrors in random order to spread the load.
export function zigSources(pin, mirrors = zigManifest.mirrors, random = Math.random) {
  const shuffled = mirrors.map(mirror => [random(), mirror]).sort((a, b) => a[0] - b[0]);
  return [
    { url: pin.url, signatureUrl: `${pin.url}.minisig`, attempts: 3 },
    ...shuffled.map(([, mirror]) => ({
      url: `${mirror}/${pin.name}?source=shellfox`,
      signatureUrl: `${mirror}/${pin.name}.minisig?source=shellfox`,
      attempts: 1,
    })),
  ];
}

export async function downloadZig(pin, { sources = zigSources(pin), fetchBytes = fetchPinnedBytes, key } = {}) {
  const failures = [];
  for (const { url, signatureUrl, attempts } of sources) {
    try {
      const signature = (await fetchBytes(signatureUrl, { attempts })).toString('utf8');
      const data = await fetchBytes(url, { attempts });
      verifyZig(pin, data, signature, key);
      return { data, signature };
    } catch (error) {
      failures.push(`${url}: ${error.message}`);
      console.warn(`Zig source failed, trying the next one: ${url}: ${error.message}`);
    }
  }
  throw new Error(`No source served a verified ${pin.name}:\n${failures.join('\n')}`);
}

// Returns verified archive bytes, from the cache when it holds a valid copy.
export async function pinnedZigArchive(pin, { cache = zigArchiveCache, download = downloadZig } = {}) {
  const archive = join(cache, pin.name);
  const signatureFile = `${archive}.minisig`;
  if (existsSync(archive) && existsSync(signatureFile)) {
    const data = readFileSync(archive);
    try {
      verifyZig(pin, data, readFileSync(signatureFile, 'utf8'));
      console.warn(`Using cached ${pin.name}`);
      return data;
    } catch (error) {
      console.warn(`Discarding cached ${pin.name}: ${error.message}`);
    }
  }
  rmSync(archive, { force: true });
  rmSync(signatureFile, { force: true });
  const { data, signature } = await download(pin);
  mkdirSync(cache, { recursive: true });
  writeFileSync(`${archive}.partial`, data);
  renameSync(`${archive}.partial`, archive);
  writeFileSync(signatureFile, signature);
  return data;
}
