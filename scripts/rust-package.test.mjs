import assert from 'node:assert/strict';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { assertLicenseTexts, lockPackages, treePackages } from './rust-package-audit.mjs';
import { standardLicense, verifyAllLicenseInputs, verifyLicenseInput } from './rust-package-licenses.mjs';

test('release notices use only a supported SPDX license declared by the dependency', () => {
  assert.match(standardLicense('MIT', ['Packager <author@example.test>']).text, /Copyright \(c\) Packager\n/);
  assert.equal(standardLicense('MIT/Apache-2.0').id, 'Apache-2.0');
  assert.equal(standardLicense('Zlib OR Apache-2.0 OR MIT').id, 'Apache-2.0');
  assert.equal(standardLicense('CC0-1.0').id, 'CC0-1.0');
  assert.equal(standardLicense('MIT AND Apache-2.0'), null);
  assert.equal(standardLicense(undefined), null);
});
import { assertAdvisoryReport, assertDatabaseAge, classifyAdvisories } from './rust-package-security.mjs';
import { assertHeadlessFiles, assertHeadlessNames, fileHash, filesUnder, root, selectedBinaries, targets } from './rust-package-common.mjs';
import { writeManifest } from './rust-package.mjs';
import { writeLaunchers } from './rust-package-resources.mjs';
import { verifyManifest } from './rust-package-smoke.mjs';

function fixture(action) {
  mkdirSync(path.join(root, 'tmp'), { recursive: true });
  const directory = mkdtempSync(path.join(root, 'tmp', 'rust-package test '));
  try { return action(directory); } finally { rmSync(directory, { recursive: true, force: true }); }
}

test('locked inventory retains real source and checksums, not assumed MIT', () => {
  const records = lockPackages(`version = 4\n\n[[package]]\nname = "gpui"\nversion = "0.2.2"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "abc123"\n\n[[package]]\nname = "shellfox-cli"\nversion = "0.1.0"\n`);
  assert.equal(records[0].checksum, 'abc123');
  assert.match(records[0].source, /^registry\+/);
  assert.equal(records[1].checksum, undefined);
  assert.throws(() => lockPackages('[[package]]\nname = "broken"\n'), /Invalid/);
  const actual = lockPackages(readFileSync(path.join(root, 'Cargo.lock'), 'utf8'));
  assert.ok(actual.some(pkg => pkg.name === 'gpui' && pkg.version === '0.2.2' && pkg.checksum.length === 64));
});

test('strict notice gate and supplemental evidence do not treat declarations as license texts', () => {
  const inputs = JSON.parse(readFileSync(path.join(root, 'scripts/rust-package-license-inputs.json'), 'utf8'));
  const result = verifyAllLicenseInputs();
  assert.equal(result.count, 44);
  assert.deepEqual(result.unresolved.sort(), [
    'convert_case@0.4.0', 'hexf-parse@0.2.1', 'mac@0.1.1', 'seahash@4.1.0', 'simd_helpers@0.1.0',
    'block@0.1.6', 'block2@0.6.2', 'leak@0.1.2', 'leaky-cow@0.1.1', 'malloc_buf@0.0.6',
    'objc2@0.6.5', 'objc2-app-kit@0.3.2', 'objc2-core-foundation@0.3.2', 'objc2-encode@4.1.0',
    'objc2-foundation@0.3.2', 'objc2-metal@0.3.2', 'objc2-quartz-core@0.3.2', 'pathfinder_geometry@0.5.1',
  ].sort());
  assert.throws(() => assertLicenseTexts({ dependencies: [{ name: 'missing', version: '1', reviewRequired: true }] }), /License review required/);
  assert.doesNotThrow(() => assertLicenseTexts({ dependencies: [{ reviewRequired: false }] }));
  const entry = inputs.dependencies.find(entry => entry.files.some(file => file.role === 'license-text'));
  assert.throws(() => verifyLicenseInput({ ...entry, files: [{ ...entry.files[0], sha256: '0'.repeat(64) }] }), /SHA-256 mismatch/);
  assert.throws(() => verifyLicenseInput({ ...entry, files: [{ ...entry.files[0], file: '../LICENSE' }] }), /Invalid supplemental/);
});

test('advisory report separates inactive optional lockfile entries from selected dependencies', () => {
  const item = (name, version, id) => ({ package: { name, version }, advisory: { id, title: 'fixture', url: null } });
  const result = classifyAdvisories({ vulnerabilities: { list: [item('rsa', '0.9.10', 'RSA'), item('libsqlite3-sys', '0.30.1', 'SQLITE')] }, warnings: { unmaintained: [item('async-std', '1.13.2', 'MAINT')] } }, new Set(['libsqlite3-sys@0.30.1']));
  assert.equal(result.vulnerabilities[0].activeInSelectedTree, false);
  assert.equal(result.vulnerabilities[1].activeInSelectedTree, true);
  assert.equal(result.warnings[0].activeInSelectedTree, false);
});

test('unavailable or filtered advisory data cannot become a passing audit', () => {
  const valid = { database: { 'advisory-count': 1 }, lockfile: { 'dependency-count': 1 },
    settings: { ignore: [], severity: null }, vulnerabilities: { found: false, count: 0, list: [] }, warnings: {} };
  assert.doesNotThrow(() => assertAdvisoryReport(valid));
  assert.throws(() => assertAdvisoryReport({ error: 'database unavailable' }), /refusing/);
  assert.throws(() => assertAdvisoryReport({ ...valid, settings: { ignore: ['RUSTSEC-example'], severity: null } }), /refusing/);
  assert.throws(() => assertAdvisoryReport({ ...valid, vulnerabilities: { count: 1, list: [] } }), /refusing/);
  assert.doesNotThrow(() => assertDatabaseAge(100, 100_000));
  assert.throws(() => assertDatabaseAge(0, 91 * 24 * 60 * 60_000), /stale/);
  assert.throws(() => assertDatabaseAge(Number.NaN), /stale/);
});

test('headless cargo tree rejects GPUI/GPU/Electron even transitive duplicates', () => {
  const tree = treePackages('shellfox-cli v0.1.0 (P:\\work\\shellfox-cli)\nserde v1.0.228\nserde v1.0.228 (*)\ngpui v0.2.2\n');
  assert.equal(tree.size, 3);
  assert.throws(() => assertHeadlessNames([...tree].map(pkg => pkg.split('@')[0])), /gpui/);
  for (const name of ['gpui', 'gpui_macros', 'wgpu-core', 'ash', 'blade-graphics', 'gpu-alloc', 'gpu-descriptor', 'metal', 'electron', 'shellfox-desktop']) {
    assert.throws(() => assertHeadlessNames([name]), /leak/);
  }
  assert.doesNotThrow(() => assertHeadlessNames(['shellfox-service', 'shellfox-host', 'shellfox-terminal', 'shellfox-ghostty-vt', 'windows-sys', 'tokio', 'serde']));
  assert.throws(() => treePackages('unexpected tree record'), /Unexpected/);
});

test('headless archive inventory rejects JS native runtime and browser GPU assets', () => {
  for (const file of ['node_modules/anything/index.js', 'bin/electron.exe', 'bin/node.exe', 'bin/node', 'app/app.asar', 'bin/libEGL.dll', 'resources/icudtl.dat', 'app/native.node', 'resources/elevated-terminal/Shellfox.ElevatedTerminal.exe']) {
    assert.throws(() => assertHeadlessFiles([file]), /leak/, file);
  }
  assert.doesNotThrow(() => assertHeadlessFiles(['bin/shellfox-cli.exe', 'bin/ghostty-vt.dll', 'resources/tailscale/shellfox-tailscale.exe', 'notices/cargo/serde-1/LICENSE']));
});

test('native matrix retains all six desktop/headless architectures', () => {
  assert.equal(Object.keys(targets).length, 6);
  assert.equal(targets['win32-arm64'], 'aarch64-pc-windows-msvc');
  assert.equal(targets['darwin-arm64'], 'aarch64-apple-darwin');
});

test('binary selection uses actual workspace targets and fails if upcoming CLI is absent', () => {
  const metadata = { workspace_members: ['cli', 'desktop'], packages: [
    { id: 'cli', name: 'shellfox-cli', version: '0.1.0', targets: [{ name: 'shellfox-cli', kind: ['bin'] }] },
    { id: 'desktop', name: 'shellfox-desktop', version: '0.1.0', targets: [{ name: 'shellfox', kind: ['bin'] }] },
  ] };
  assert.equal(selectedBinaries(metadata, 'desktop').length, 2);
  assert.equal(selectedBinaries(metadata, 'headless').length, 1);
  assert.throws(() => selectedBinaries({ workspace_members: [], packages: [] }, 'headless'), /not available/);
});

test('manifest covers notices, binaries, hashes and explicitly unsupported updates', () => fixture(directory => {
  mkdirSync(path.join(directory, 'bin'));
  writeFileSync(path.join(directory, 'bin', 'shellfox-cli'), 'fixture only');
  writeFileSync(path.join(directory, 'THIRD-PARTY-NOTICES.txt'), 'fixture notices');
  writeManifest(directory, { kind: 'headless', platform: process.platform, arch: process.arch });
  const manifest = verifyManifest(directory);
  assert.equal(manifest.files.length, 2);
  assert.equal(manifest.update.supported, false);
  assert.equal(manifest.dataSelection.argument, '--data-dir');
  writeFileSync(path.join(directory, 'THIRD-PARTY-NOTICES.txt'), 'tampered');
  assert.throws(() => verifyManifest(directory), /Changed package file/);
}));

test('trusted producer returns exact manifest digest and publishes only an external anchor', () => fixture(directory => {
  const payload = path.join(directory, 'payload');
  mkdirSync(payload);
  writeFileSync(path.join(payload, 'LICENSE'), 'fixture only');
  const anchor = path.join(directory, 'archive.package-manifest.sha256');
  const produced = writeManifest(payload, { kind: 'headless', platform: process.platform, arch: process.arch }, anchor);
  const manifestFile = path.join(payload, 'package-manifest.json');
  assert.equal(produced.expectedManifestDigest, fileHash(manifestFile));
  assert.deepEqual(produced.manifest, JSON.parse(readFileSync(manifestFile, 'utf8')));
  assert.equal(readFileSync(anchor, 'utf8'), `${produced.expectedManifestDigest}  package-manifest.json\n`);
  assert.equal(produced.manifest.format, 'shellfox-rust-portable-v1');
  assert.equal(produced.manifest.expectedManifestDigest, undefined);
  assert.deepEqual(produced.manifest.files.map(file => file.path), ['LICENSE']);
  assert.equal(verifyManifest(payload).files.length, 1);
  const original = readFileSync(manifestFile, 'utf8');
  assert.throws(() => writeManifest(payload, { kind: 'changed' }, anchor), /overwrite/);
  assert.equal(readFileSync(manifestFile, 'utf8'), original);
}));

test('producer refuses self-contained or relative runtime trust anchors before writing', () => fixture(directory => {
  writeFileSync(path.join(directory, 'LICENSE'), 'fixture only');
  assert.throws(() => writeManifest(directory, {}, path.join(directory, 'self.sha256')), /outside/);
  assert.throws(() => writeManifest(directory, {}, 'relative.sha256'), /absolute/);
  assert.deepEqual(filesUnder(directory), ['LICENSE']);
}));

test('inventory rejects path traversal, duplicates and unmanifested files', () => fixture(directory => {
  writeFileSync(path.join(directory, 'LICENSE'), 'fixture');
  writeManifest(directory, { kind: 'headless', platform: process.platform, arch: process.arch });
  const file = path.join(directory, 'package-manifest.json');
  const original = JSON.parse(readFileSync(file, 'utf8'));
  for (const bad of ['../LICENSE', '/LICENSE', 'C:/LICENSE', 'dir\\LICENSE', 'dir//LICENSE']) {
    writeFileSync(file, JSON.stringify({ ...original, files: [{ path: bad, sha256: fileHash(path.join(directory, 'LICENSE')) }] }));
    assert.throws(() => verifyManifest(directory));
  }
  writeFileSync(file, JSON.stringify({ ...original, files: [...original.files, ...original.files] }));
  assert.throws(() => verifyManifest(directory), /Duplicate/);
  writeFileSync(file, JSON.stringify(original));
  writeFileSync(path.join(directory, 'unlisted'), 'extra');
  assert.throws(() => verifyManifest(directory), /Unmanifested/);
}));

test('launchers retain argument forwarding without environment resource magic or Node', () => fixture(directory => {
  writeLaunchers(directory, 'desktop');
  const contents = filesUnder(directory).map(file => readFileSync(path.join(directory, file), 'utf8')).join('\n');
  assert.match(contents, /shellfox-cli/);
  assert.match(contents, /shellfox/);
  assert.doesNotMatch(contents, /ELECTRON|NODE_|SHELLFOX_RESOURCES|USER_DATA/);
  assert.match(contents, process.platform === 'win32' ? /@args/ : /"\$@"/);
  if (process.platform !== 'win32') chmodSync(path.join(directory, 'shellfox'), 0o755);
}));

test('new workflow is manual, read-only and cannot publish or sign', () => {
  const workflow = readFileSync(path.join(root, '.github/workflows/rust-platforms.yml'), 'utf8');
  assert.match(workflow, /workflow_dispatch:/);
  assert.match(workflow, /contents: read/);
  assert.match(workflow, /default: false/);
  assert.doesNotMatch(workflow, /^\s+(?:push|pull_request|release|schedule):/m);
  assert.doesNotMatch(workflow, /contents: write|action-gh-release|codesign |signtool |gh release|secrets\./);
  for (const match of workflow.matchAll(/uses: ([^\s]+)/g)) assert.match(match[1], /@[a-f0-9]{40}$/);
});
