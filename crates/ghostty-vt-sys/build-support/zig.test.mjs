import assert from 'node:assert/strict';
import { createHash, generateKeyPairSync, sign } from 'node:crypto';
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';
import { project } from './archives.mjs';
import { downloadZig, pinnedZigArchive, verifyZig, zigManifest, zigPin, zigSources } from './zig.mjs';

// A throwaway minisign key, so tests can sign fixtures the way Zig signs its archives.
const { publicKey, privateKey } = generateKeyPairSync('ed25519');
const keyId = Buffer.from('0123456789abcdef', 'hex');
const key = Buffer.concat([Buffer.from('Ed'), keyId,
  publicKey.export({ format: 'der', type: 'spki' }).subarray(12)]).toString('base64');

function minisign(data, file) {
  const signature = sign(null, createHash('blake2b512').update(data).digest(), privateKey);
  const comment = `timestamp:1\tfile:${file}\thashed`;
  const global = sign(null, Buffer.concat([signature, Buffer.from(comment)]), privateKey);
  return ['untrusted comment: test', Buffer.concat([Buffer.from('ED'), keyId, signature]).toString('base64'),
    `trusted comment: ${comment}`, global.toString('base64')].join('\n');
}

const data = Buffer.from('zig fixture');
const pin = { url: 'https://ziglang.org/download/0.17.0/zig-x86_64-linux-0.17.0.tar.xz',
  name: 'zig-x86_64-linux-0.17.0.tar.xz', sha256: createHash('sha256').update(data).digest('hex') };

test('every pinned host resolves and the mirror list is HTTPS', () => {
  for (const [platform, arch] of [['win32', 'x64'], ['win32', 'arm64'], ['linux', 'x64'],
    ['linux', 'arm64'], ['darwin', 'x64'], ['darwin', 'arm64']]) {
    assert.match(zigPin(platform, arch).name, /^zig-.*-0\.17\.0\.(?:zip|tar\.xz)$/);
  }
  assert.ok(zigManifest.mirrors.length > 0);
  for (const mirror of zigManifest.mirrors) assert.match(mirror, /^https:\/\/[^?#]+[^/]$/);
});

test('ziglang.org is tried first with retries, then each mirror once', () => {
  const sources = zigSources(pin, ['https://a.example/zig', 'https://b.example'], () => 0.5);
  assert.deepEqual(sources.map(source => [source.url, source.attempts]), [
    [pin.url, 3],
    ['https://a.example/zig/zig-x86_64-linux-0.17.0.tar.xz?source=shellfox', 1],
    ['https://b.example/zig-x86_64-linux-0.17.0.tar.xz?source=shellfox', 1],
  ]);
  assert.equal(sources[1].signatureUrl, 'https://a.example/zig/zig-x86_64-linux-0.17.0.tar.xz.minisig?source=shellfox');
});

test('a Zig archive needs the pinned hash and a signature for its own file name', () => {
  verifyZig(pin, data, minisign(data, pin.name), key);
  assert.throws(() => verifyZig(pin, Buffer.from('other'), minisign(data, pin.name), key), /SHA-256 mismatch/);
  assert.throws(() => verifyZig(pin, data, minisign(data, 'zig-other.tar.xz'), key), /another file/);
  assert.throws(() => verifyZig(pin, data, minisign(Buffer.from('other'), pin.name), key), /verification failed/);
  assert.throws(() => verifyZig(pin, data, minisign(data, pin.name)), /signing key/);
});

test('a source that fails or serves unverifiable bytes is skipped, never used', async () => {
  const sources = ['https://down.example', 'https://bad.example', 'https://good.example']
    .map(base => ({ url: `${base}/zig`, signatureUrl: `${base}/zig.minisig`, attempts: 1 }));
  const fetched = [];
  const result = await downloadZig(pin, {
    sources, key,
    async fetchBytes(url) {
      fetched.push(url);
      if (url.startsWith('https://down.example')) throw new Error('timed out');
      if (url.endsWith('.minisig')) return Buffer.from(minisign(data, pin.name));
      return url.startsWith('https://bad.example') ? Buffer.from('tampered') : data;
    },
  });
  assert.equal(result.data.toString(), 'zig fixture');
  assert.equal(fetched.length, 5);
  await assert.rejects(downloadZig(pin, { sources: sources.slice(0, 2), key, async fetchBytes(url) {
    return url.endsWith('.minisig') ? Buffer.from(minisign(data, pin.name)) : Buffer.from('tampered');
  } }), /No source served a verified/);
});

test('a cached archive that fails verification is replaced by a fresh verified download', async () => {
  mkdirSync(join(project, 'tmp'), { recursive: true });
  const cache = mkdtempSync(join(project, 'tmp', 'zig-cache-test-'));
  try {
    writeFileSync(join(cache, pin.name), 'corrupt');
    writeFileSync(join(cache, `${pin.name}.minisig`), 'corrupt');
    let downloads = 0;
    const download = async () => { downloads++; throw new Error('offline'); };
    await assert.rejects(pinnedZigArchive(pin, { cache, download }), /offline/);
    assert.equal(downloads, 1);
    assert.equal(existsSync(join(cache, pin.name)), false);
  } finally { rmSync(cache, { recursive: true, force: true }); }
});
