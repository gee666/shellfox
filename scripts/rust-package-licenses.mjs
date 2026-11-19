import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { fileHash, readJson, root } from './rust-package-common.mjs';

const inputs = path.join(root, 'scripts/rust-package-license-inputs.json');
const texts = path.join(root, 'scripts/rust-package-license-texts');

export function verifyLicenseInput(entry) {
  if (!/^[a-f0-9]{40}$/.test(entry.commit) || !/^[a-f0-9]{64}$/.test(entry.crateChecksum)) {
    throw new Error(`Invalid license provenance: ${entry.name}`);
  }
  for (const file of entry.files) {
    if (!file.file.startsWith(`${entry.name}-${entry.version}/`) || file.file.includes('\\') ||
        file.file.split('/').some(part => !part || part === '.' || part === '..') ||
        !['license-text', 'declaration'].includes(file.role) || !/^[a-f0-9]{64}$/.test(file.sha256)) {
      throw new Error(`Invalid supplemental license file: ${file.file}`);
    }
    const url = new URL(file.url);
    if (url.protocol !== 'https:' || !['raw.githubusercontent.com', 'gitlab.redox-os.org'].includes(url.hostname) || !url.pathname.includes(entry.commit)) {
      throw new Error(`Unpinned supplemental license URL: ${file.url}`);
    }
    const bytes = readFileSync(path.join(texts, file.file));
    if (fileHash(path.join(texts, file.file)) !== file.sha256) throw new Error(`License SHA-256 mismatch: ${file.file}`);
    const blob = createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex');
    if (blob !== file.gitBlob) throw new Error(`License Git blob mismatch: ${file.file}`);
  }
}

export function supplementalLicense(pkg, locked, destination) {
  const manifest = readJson(inputs);
  if (manifest.format !== 'shellfox-rust-supplemental-licenses-v1') throw new Error('Unknown supplemental license format');
  const entry = manifest.dependencies.find(entry => entry.name === pkg.name && entry.version === pkg.version);
  if (!entry) return null;
  if (entry.crateChecksum !== locked.checksum || entry.license !== pkg.license) {
    throw new Error(`Supplemental license no longer matches locked crate: ${pkg.name}@${pkg.version}`);
  }
  verifyLicenseInput(entry);
  mkdirSync(destination, { recursive: true });
  const files = entry.files.map((file, index) => {
    const name = `upstream-${index}-${path.basename(file.file)}`;
    copyFileSync(path.join(texts, file.file), path.join(destination, name));
    return { name, sha256: file.sha256, role: file.role, url: file.url };
  });
  writeFileSync(path.join(destination, 'upstream-provenance.json'), JSON.stringify(entry, null, 2) + '\n');
  return { files, selectedLicense: entry.selectedLicense ?? null, unresolvedReason: entry.unresolvedReason ?? null };
}

// Use the published SPDX expression, never guess a missing expression or ignore AND.
// For dual-licensed crates select Apache-2.0, which does not need a copyright template.
export function standardLicense(expression, authors = []) {
  const options = (expression ?? '').trim().split(/\s+OR\s+|\s*\/\s*/);
  if (!options.every(option => /^[a-z\d.-]+$/i.test(option))) return null;
  const id = ['Apache-2.0', 'MIT', 'CC0-1.0'].find(candidate => options.includes(candidate));
  if (!id) return null;
  let text = readFileSync(path.join(texts, 'spdx', id + '.txt'), 'utf8');
  if (id === 'MIT' && authors.length) {
    const attribution = authors.map(author => author.replace(/\s*<[^>]+>/g, '')).join(', ');
    text = text.replace('Copyright (c) <year> <copyright holders>', `Copyright (c) ${attribution}`);
  }
  return { id, text };
}

export function verifyAllLicenseInputs() {
  const manifest = readJson(inputs);
  for (const entry of manifest.dependencies) verifyLicenseInput(entry);
  return { count: manifest.dependencies.length, unresolved: manifest.dependencies.filter(entry => !entry.files.some(file => file.role === 'license-text')).map(entry => `${entry.name}@${entry.version}`) };
}
