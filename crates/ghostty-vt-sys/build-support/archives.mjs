import { createHash, createPublicKey, verify } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';

export const support = dirname(fileURLToPath(import.meta.url));
export const project = resolve(support, '../../..');
export const vendor = join(support, 'vendor');
export const version = '1.0.1';
export const zigVersion = '0.17.0';
export const release = `https://release.files.ghostty.org/libghostty-vt/${version}/`;
export const publicKey = 'RWQlAjJC23149WL2sEpT/l0QKy7hMIFhYdQOFy0Z7z7PbneUgvlsnYcV';
export const archives = Object.freeze({
  'ghostty-vt-1.0.1-x86_64-windows-gnu.zip': '8e2eb9e1822e85d786b456c8beb86f49d09c318c91f0c2913f91e7ffa12cda29',
  'libghostty-vt-1.0.1.tar.gz': '333dfce97641ef9b536bb9e08907ad86abaaada50e8f7417b70d9284adef37aa',
  'libghostty-vt-1.0.1-deps.tar.gz': '91a4e10960a52bf5bf4b15aa2a2cddaff8a89d6cd903d3055bda4fd17de3356d',
});

export async function fetchPinnedBytes(url, { request = fetch, pause = delay } = {}) {
  let last;
  for (let attempt = 1; attempt <= 3; attempt++) {
    try {
      const response = await request(url, { signal: AbortSignal.timeout(120_000) });
      if (response.ok) return Buffer.from(await response.arrayBuffer());
      const error = new Error(`Download failed: HTTP ${response.status} for ${url}`);
      if (response.status !== 429 && response.status < 500) throw error;
      last = error;
    } catch (error) {
      if (error?.name !== 'TimeoutError' && error?.name !== 'TypeError') throw error;
      last = error;
    }
    if (attempt < 3) {
      console.warn(`Retrying pinned archive download ${url} after ${last.message}`);
      await pause(attempt * 1_000);
    }
  }
  throw last;
}

export function verifyArchive(name, data) {
  const expected = archives[name];
  if (!expected) throw new Error(`Unpinned archive: ${name}`);
  if (createHash('sha256').update(data).digest('hex') !== expected) {
    throw new Error(`SHA-256 mismatch: ${name}`);
  }
}

// Support the hashed Ed25519 minisign format used by this release, not arbitrary signatures.
export function verifyManifest(message, signature) {
  const lines = signature.trimEnd().split(/\r?\n/);
  const key = Buffer.from(publicKey, 'base64');
  const packet = Buffer.from(lines[1] ?? '', 'base64');
  const globalSignature = Buffer.from(lines[3] ?? '', 'base64');
  if (lines.length !== 4 || packet.length !== 74 || packet.toString('ascii', 0, 2) !== 'ED'
      || !packet.subarray(2, 10).equals(key.subarray(2, 10))
      || !lines[2].startsWith('trusted comment: ') || globalSignature.length !== 64) {
    throw new Error('Unexpected upstream minisign format or signing key');
  }
  const ed25519 = createPublicKey({
    key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), key.subarray(10)]),
    format: 'der', type: 'spki',
  });
  const digest = createHash('blake2b512').update(message).digest();
  const commentMessage = Buffer.concat([packet.subarray(10), Buffer.from(lines[2].slice(17))]);
  if (!verify(null, digest, ed25519, packet.subarray(10))
      || !verify(null, commentMessage, ed25519, globalSignature)) {
    throw new Error('Upstream minisign signature verification failed');
  }
  const checksums = new Map(message.toString('utf8').trim().split(/\r?\n/).map(line => {
    const match = /^([a-f0-9]{64})  (\S+)$/.exec(line);
    if (!match) throw new Error('Invalid upstream SHA256SUMS entry');
    return [match[2], match[1]];
  }));
  for (const [name, hash] of Object.entries(archives)) {
    if (checksums.get(name) !== hash) throw new Error(`Signed manifest disagrees with pin: ${name}`);
  }
}

export function verifyInputs(names = Object.keys(archives)) {
  verifyManifest(readFileSync(join(vendor, 'SHA256SUMS')),
    readFileSync(join(vendor, 'SHA256SUMS.minisig'), 'utf8'));
  for (const name of names) verifyArchive(name, readFileSync(join(vendor, name)));
}
