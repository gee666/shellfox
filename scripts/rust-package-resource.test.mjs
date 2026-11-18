import assert from 'node:assert/strict';
import { linkSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileHash, root } from './rust-package-common.mjs';
import { generateResourceManifest, validateResourcePath } from './rust-package-resource-manifest.mjs';
import { auditManagedInventory, managedInventory, nugetRangeContains } from './rust-package-resource-inventory.mjs';

function fixture(action, runtimeVersion = '8.0.31') {
  const directory = mkdtempSync(path.join(root, 'tmp/rust-package-resource test '));
  const publish = path.join(directory, 'publish');
  mkdirSync(publish);
  for (const name of ['Shellfox.ElevatedTerminal.exe', 'Shellfox.ElevatedTerminal.dll', 'hostfxr.dll', 'hostpolicy.dll', 'coreclr.dll', 'clrjit.dll', 'System.Private.CoreLib.dll', 'extra-build-native.dll']) {
    writeFileSync(path.join(publish, name), `synthetic bytes: ${name}`);
  }
  const framework = runtimeVersion.split('.').slice(0, 2).join('.');
  const target = `.NETCoreApp,Version=v${framework}/win-x64`;
  writeFileSync(path.join(publish, 'Shellfox.ElevatedTerminal.deps.json'), JSON.stringify({
    runtimeTarget: { name: target },
    targets: { [target]: { [`runtimepack.Microsoft.NETCore.App.Runtime.win-x64/${runtimeVersion}`]: {
      runtime: { 'System.Private.CoreLib.dll': { assemblyVersion: `${framework}.0.0`, fileVersion: `${runtimeVersion}.1` } }, native: { 'coreclr.dll': {} },
    } } },
    libraries: { 'Shellfox.ElevatedTerminal/1.0.0': { type: 'project' }, [`runtimepack.Microsoft.NETCore.App.Runtime.win-x64/${runtimeVersion}`]: { type: 'runtimepack' } },
  }));
  writeFileSync(path.join(publish, 'Shellfox.ElevatedTerminal.runtimeconfig.json'), JSON.stringify({ runtimeOptions: { tfm: `net${framework}`, includedFrameworks: [{ name: 'Microsoft.NETCore.App', version: runtimeVersion }] } }));
  const options = { directory: publish, kind: 'managed', rid: 'win-x64', version: '0.1.0-fixture', runtimeVersion, provenance: path.join(directory, 'trusted-build.json') };
  try { return action({ directory, publish, options }); } finally { rmSync(directory, { recursive: true, force: true }); }
}

test('trusted producer binds the whole publish, not just an unchanged apphost', () => fixture(({ publish, options }) => {
  const record = generateResourceManifest(options);
  const manifest = JSON.parse(readFileSync(path.join(publish, 'bundle-manifest.json'), 'utf8'));
  assert.equal(fileHash(path.join(publish, 'bundle-manifest.json')), record.expectedManifestDigest);
  assert.equal(manifest.runtime.selfContained, true);
  assert.ok(manifest.files.some(file => file.path === 'Shellfox.ElevatedTerminal.dll'));
  assert.ok(manifest.files.some(file => file.path === 'extra-build-native.dll'));
  assert.ok(manifest.files.some(file => file.path === 'managed-inventory.json'));
  assert.ok(manifest.files.some(file => file.path === 'managed-vulnerability-report.json'));
  const originalExe = fileHash(path.join(publish, 'Shellfox.ElevatedTerminal.exe'));
  writeFileSync(path.join(publish, 'Shellfox.ElevatedTerminal.dll'), 'tampered managed assembly');
  assert.equal(fileHash(path.join(publish, 'Shellfox.ElevatedTerminal.exe')), originalExe);
  assert.notEqual(fileHash(path.join(publish, 'Shellfox.ElevatedTerminal.dll')), manifest.files.find(file => file.path.endsWith('ElevatedTerminal.dll')).sha256);
  assert.throws(() => generateResourceManifest(options), /overwrite/);
}));

test('supported net10 publish inventory and manifest use the actual declared framework', () => fixture(({ publish, options }) => {
  generateResourceManifest(options);
  const manifest = JSON.parse(readFileSync(path.join(publish, 'bundle-manifest.json'), 'utf8'));
  assert.equal(manifest.runtime.targetFramework, 'net10.0-windows');
  assert.equal(manifest.runtime.version, '10.0.15');
  assert.throws(() => managedInventory(publish, { rid: 'win-x64', runtimeVersion: '8.0.31' }), /differs/);
}, '10.0.15'));

test('producer requires outside provenance and complete runtime', () => {
  fixture(({ publish, options }) => assert.throws(() => generateResourceManifest({ ...options, provenance: path.join(publish, 'self-trust.json') }), /outside/));
  fixture(({ publish, options }) => { rmSync(path.join(publish, 'coreclr.dll')); assert.throws(() => generateResourceManifest(options), /asset missing/); });
  fixture(({ options }) => assert.throws(() => generateResourceManifest({ ...options, rid: 'osx-arm64' }), /mismatch/));
  fixture(({ publish, options }) => {
    writeFileSync(path.join(publish, 'Shellfox.ElevatedTerminal.runtimeconfig.dev.json'), '{}');
    assert.throws(() => generateResourceManifest(options), /Development loader probing/);
  });
});

test('producer refuses symlink/junction and hardlink aliases', () => {
  fixture(({ directory, publish, options }) => {
    linkSync(path.join(publish, 'coreclr.dll'), path.join(directory, 'external-alias.dll'));
    assert.throws(() => generateResourceManifest(options), /alias/);
  });
  fixture(({ directory, publish, options }) => {
    const target = path.join(directory, 'outside'); mkdirSync(target);
    writeFileSync(path.join(target, 'late.dll'), 'synthetic');
    symlinkSync(target, path.join(publish, 'redirect'), process.platform === 'win32' ? 'junction' : 'dir');
    assert.throws(() => generateResourceManifest(options), /link\/reparse/);
  });
});

test('resource relative paths reject traversal, streams and Windows aliases on every OS', () => {
  for (const name of ['../x.dll', '/x.dll', 'a\\x.dll', 'C:x.dll', 'x/../y', 'a//b', 'a.', 'CON.dll', 'com1.exe', 'x y.dll']) assert.throws(() => validateResourcePath(name), undefined, name);
  assert.doesNotThrow(() => validateResourcePath('runtimes/win-arm64/native/coreclr.dll'));
});

test('managed inventory binds RID/runtime and records actual package and assembly versions', () => fixture(({ publish }) => {
  const inventory = managedInventory(publish, { rid: 'win-x64', runtimeVersion: '8.0.31' });
  assert.equal(inventory.packages.find(pkg => pkg.kind === 'runtimepack').version, '8.0.31');
  assert.equal(inventory.assets.find(asset => asset.path === 'System.Private.CoreLib.dll').assemblyVersion, '8.0.0.0');
  assert.throws(() => managedInventory(publish, { rid: 'win-arm64', runtimeVersion: '8.0.31' }), /differs/);
}));

test('NuGet reports are separate from Cargo and unknown ranges do not silently pass', () => {
  assert.equal(nugetRangeContains('8.0.31', '[8.0.0, 8.0.32)'), true);
  assert.equal(nugetRangeContains('8.0.32', '[8.0.0,8.0.32)'), false);
  assert.equal(nugetRangeContains('1.2.3', '[1.2.3]'), true);
  assert.equal(nugetRangeContains('1.2.3', '(,1.2.3)'), false);
  assert.equal(nugetRangeContains('1.2.3-preview', '[1.0.0,2.0.0)'), true);
  assert.equal(nugetRangeContains('8.0.31', '[11.0.0-preview.1,11.0.0)'), false);
  assert.equal(nugetRangeContains('8.0.31', '[7.0.0-preview.1.22076.8,7.0.16]'), false);
  assert.equal(nugetRangeContains('1.2.3', '^1.0.0'), null);
  const inventory = { packages: [{ name: 'Example.Native', version: '1.2.3', kind: 'package' }] };
  assert.equal(auditManagedInventory(inventory).status, 'not-run');
  const lifecycle = auditManagedInventory({ ...inventory, runtimeVersion: '8.0.31' }, {
    format: 'shellfox-nuget-snapshot-v1', packages: {}, sources: [],
    dotnet: { channel: '8.0', supportPhase: 'eol', latestRuntime: '8.0.34', eolDate: '2026-11-10' },
  });
  assert.equal(lifecycle.runtimeLifecycle.status, 'unsupported-channel');
  const snapshot = { format: 'shellfox-nuget-snapshot-v1', sources: [], packages: { 'example.native': [{ url: 'https://example.invalid/advisory', severity: 2, versions: '[1.0.0,2.0.0)' }] } };
  assert.equal(auditManagedInventory(inventory, snapshot).status, 'findings');
  snapshot.packages['example.native'][0].versions = 'unknown-future-range';
  const report = auditManagedInventory(inventory, snapshot);
  assert.equal(report.status, 'incomplete');
  assert.match(report.coverage, /Does not assert/);
});
