import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { root } from './rust-package-common.mjs';
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
  try { return action({ publish }); } finally { rmSync(directory, { recursive: true, force: true }); }
}

test('net10 publish inventory uses the actual declared framework', () => fixture(({ publish }) => {
  const inventory = managedInventory(publish, { rid: 'win-x64', runtimeVersion: '10.0.15' });
  assert.equal(inventory.packages.find(pkg => pkg.kind === 'runtimepack').version, '10.0.15');
  assert.throws(() => managedInventory(publish, { rid: 'win-x64', runtimeVersion: '8.0.31' }), /differs/);
}, '10.0.15'));

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
