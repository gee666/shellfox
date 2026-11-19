import { closeSync, existsSync, lstatSync, mkdirSync, openSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { parseArgs } from 'node:util';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { helperProtocolVersion } from './tailscale.mjs';
import { run, sha256 } from './rust-package-common.mjs';
import { acquireNugetSnapshot, managedInventory, writeManagedAudit } from './rust-package-resource-inventory.mjs';

export const bundleManifestName = 'bundle-manifest.json';
const rids = new Set(['win-x64', 'win-arm64', 'linux-x64', 'linux-arm64', 'osx-x64', 'osx-arm64']);
const baseCaps = {
  'elevated-conpty': ['conpty'], 'elevated-isolated': ['isolated-windows-power-shell', 'no-profile'],
  'power-guardian': ['display', 'system', 'away-mode', 'idle-mouse', 'lid-rollback'],
  'darwin-snapshot': ['exact-birth', 'identity-access', 'enumeration', 'argv', 'environment-marker'],
  'darwin-supervisor': ['owned-session', 'guardians', 'termination'], 'wsl-guardian': ['pidfd', 'subreaper', 'peercred'],
};
export function contract(id, rid) {
  const capabilities = id === 'tailscale' ? ['embedded-network', 'ownership', ...(rid.startsWith('win-') ? ['private-permissions'] : [])] : baseCaps[id];
  if (!capabilities) throw new Error('Unknown native protocol');
  return { id, version: 1, capabilities: [...capabilities] };
}

export function validateResourcePath(name) {
  if (!name || name.length > 512 || name.split('/').length > 16) throw new Error('Invalid resource relative path');
  for (const part of name.split('/')) {
    const stem = part.split('.')[0];
    if (!/^[A-Za-z0-9._-]{1,128}$/.test(part) || ['.', '..'].includes(part) || part.endsWith('.') ||
        /^(CON|PRN|AUX|NUL|COM\d|LPT\d)$/i.test(stem)) throw new Error(`Ambiguous resource path: ${name}`);
  }
}

function inventoryFiles(root, directory = root, result = [], folded = new Set()) {
  for (const entry of readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
    const full = path.join(directory, entry.name);
    const relative = path.relative(root, full).split(path.sep).join('/');
    validateResourcePath(relative);
    const stat = lstatSync(full);
    if (stat.isSymbolicLink()) throw new Error(`Resource link/reparse point refused: ${relative}`);
    if (stat.isDirectory()) {
      if (!readdirSync(full).length) throw new Error(`Unlisted empty resource directory: ${relative}`);
      inventoryFiles(root, full, result, folded);
    } else {
      if (!stat.isFile() || stat.nlink !== 1 || stat.size > 256 * 1024 * 1024 || folded.has(relative.toLowerCase())) throw new Error(`Resource file/alias bound: ${relative}`);
      folded.add(relative.toLowerCase());
      const handle = openSync(full, 'r');
      try {
        const bytes = readFileSync(handle);
        const after = lstatSync(full);
        if (stat.dev !== after.dev || stat.ino !== after.ino || stat.size !== after.size || stat.mtimeMs !== after.mtimeMs) throw new Error('Trusted build output changed during inventory');
        result.push({ path: relative, size: bytes.length, sha256: sha256(bytes) });
      } finally { closeSync(handle); }
      if (result.length > 2048) throw new Error('Resource file count exceeds bound');
    }
  }
  return result;
}

function descriptor(kind, rid, version, runtimeVersion) {
  const windows = rid.startsWith('win-');
  if (kind === 'managed' && windows) return {
    identity: { id: 'windows-managed-helper', rid, version },
    runtime: { kind: 'dotnet', version: runtimeVersion, targetFramework: `net${runtimeVersion.split('.').slice(0, 2).join('.')}-windows`, entryAssembly: 'Shellfox.ElevatedTerminal.dll', selfContained: true },
    entrypoints: [{ path: 'Shellfox.ElevatedTerminal.exe', protocols: ['elevated-conpty', 'elevated-isolated', 'power-guardian'].map(id => contract(id, rid)) }],
  };
  if (kind === 'go') return { identity: { id: 'go-tailscale', rid, version }, runtime: { kind: 'go', version: runtimeVersion },
    entrypoints: [{ path: `shellfox-tailscale${windows ? '.exe' : ''}`, protocols: [contract('tailscale', rid)] }] };
  if (kind === 'darwin' && rid.startsWith('osx-')) return { identity: { id: 'darwin-terminal', rid, version }, runtime: { kind: 'native', abi: 'darwin-libproc-v1' },
    entrypoints: [{ path: 'shellfox-process-snapshot', protocols: [contract('darwin-snapshot', rid)] }, { path: 'shellfox-terminal-supervisor', protocols: [contract('darwin-supervisor', rid)] }] };
  if (kind === 'wsl' && rid.startsWith('linux-')) return { identity: { id: 'wsl-supervisor', rid, version }, runtime: { kind: 'python', major: 3 },
    entrypoints: [{ path: 'guardian.py', protocols: [contract('wsl-guardian', rid)] }] };
  throw new Error('Resource kind/RID mismatch');
}

export function safeProbe(directory, kind, rid) {
  const env = Object.fromEntries(Object.keys(process.env).filter(key => /^(DOTNET_|COMPLUS_|CORECLR_|COR_|DEVPATH$)/i.test(key)).map(key => [key, undefined]));
  const options = { timeout: 15_000, cwd: directory, env };
  const executable = path.join(directory, kind === 'managed' ? 'Shellfox.ElevatedTerminal.exe' : `shellfox-tailscale${process.platform === 'win32' ? '.exe' : ''}`);
  if (kind === 'managed') {
    if (run(executable, ['probe'], options).trim() !== 'shellfox-elevated-conpty-v1') throw new Error('Managed ConPTY probe mismatch');
    const isolated = JSON.parse(run(executable, ['probe-isolated'], options));
    const power = JSON.parse(run(executable, ['probe-power'], options));
    assert.deepEqual(isolated, { protocol: 'shellfox-elevated-isolated', version: 1, shell: 'windows-powershell', noProfile: true });
    assert.deepEqual(power, { protocol: 'shellfox-power-guardian', version: 1, display: true, system: true, awayMode: true, idleMouse: true, lidRollback: true });
  } else if (kind === 'go') {
    const value = JSON.parse(run(executable, ['--probe'], options));
    assert.deepEqual(value, { arch: rid.endsWith('arm64') ? 'arm64' : 'amd64',
      platform: rid.startsWith('win-') ? 'windows' : rid.startsWith('osx-') ? 'darwin' : 'linux',
      protocolVersion: helperProtocolVersion, tailscaleVersion: '1.104.2' });
  } else throw new Error('This generator does not launch Darwin or WSL scripts');
}

export function generateResourceManifest({ directory, kind, rid, version, runtimeVersion, provenance, nugetSnapshot = null, probe = false }) {
  if (!path.isAbsolute(directory) || !path.isAbsolute(provenance) || !rids.has(rid) || !/^[A-Za-z0-9][A-Za-z0-9.+-]{0,63}$/.test(version)) throw new Error('Explicit absolute trusted-build output/provenance and valid identity required');
  directory = path.resolve(directory); provenance = path.resolve(provenance);
  const relative = path.relative(directory, provenance);
  if (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative)) throw new Error('Trust-anchor provenance must be outside the mutable payload directory');
  if (existsSync(path.join(directory, bundleManifestName)) || existsSync(provenance)) throw new Error('Refusing to overwrite native bundle manifest/provenance');
  if (lstatSync(directory).isSymbolicLink()) throw new Error('Trusted build root cannot be a link');
  if (['managed', 'go'].includes(kind) && !/^\d+\.\d+\.\d+$/.test(runtimeVersion ?? '')) throw new Error('Exact managed/Go runtime version required');
  const manifest = { format: 'shellfox-native-bundle-v1', ...descriptor(kind, rid, version, runtimeVersion) };
  let managedReport = null, managedAssets = [];
  if (kind === 'managed') {
    const inventory = managedInventory(directory, { rid, runtimeVersion });
    managedReport = writeManagedAudit(directory, inventory, nugetSnapshot);
    managedAssets = inventory.assets;
  }
  if (probe) safeProbe(directory, kind, rid);
  const files = inventoryFiles(directory).sort((a, b) => a.path.localeCompare(b.path));
  for (const asset of managedAssets) if (!files.some(file => file.path === asset.path || file.path === path.posix.basename(asset.path))) throw new Error(`Published dependency asset missing: ${asset.path}`);
  if (files.some(file => file.path.toLowerCase() === bundleManifestName)) throw new Error('Reserved bundle manifest alias');
  if (files.some(file => file.path.toLowerCase().endsWith('.runtimeconfig.dev.json'))) throw new Error('Development loader probing configuration refused');
  if (files.reduce((total, file) => total + file.size, 0) > 2 * 1024 * 1024 * 1024) throw new Error('Resource bundle exceeds total byte bound');
  for (const entry of manifest.entrypoints) if (!files.some(file => file.path === entry.path)) throw new Error('Required entrypoint missing');
  if (kind === 'managed') for (const name of ['Shellfox.ElevatedTerminal.dll', 'hostfxr.dll', 'hostpolicy.dll', 'coreclr.dll', 'clrjit.dll', 'System.Private.CoreLib.dll']) {
    if (!files.some(file => file.path === name)) throw new Error(`Required complete managed publish asset missing: ${name}`);
  }
  if (kind === 'wsl') for (const name of ['guardian.py', 'bootstrap.py', 'tree.py', 'wire.py']) if (!files.some(file => file.path === name)) throw new Error('Required WSL module missing');
  const bytes = Buffer.from(JSON.stringify({ ...manifest, files }, null, 2) + '\n');
  if (bytes.length > 2 * 1024 * 1024) throw new Error('Resource manifest exceeds bound');
  writeFileSync(path.join(directory, bundleManifestName), bytes, { flag: 'wx' });
  const record = { format: 'shellfox-trusted-build-resource-record-v1', identity: manifest.identity,
    expectedManifestDigest: sha256(bytes), manifest: bundleManifestName, files: files.length,
    managedAuditStatus: managedReport?.status ?? 'not-applicable', managedRuntimeLifecycleStatus: managedReport?.runtimeLifecycle.status ?? 'not-applicable', safeProbeStatus: probe ? 'matched-declared-probe' : 'not-run',
    trust: 'This record is produced by an explicitly trusted BUILD. SHA-256 is not publisher authentication. Never obtain an expected runtime anchor by hashing an untrusted manifest.' };
  mkdirSync(path.dirname(provenance), { recursive: true });
  writeFileSync(provenance, JSON.stringify(record, null, 2) + '\n', { flag: 'wx' });
  return record;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { values, positionals } = parseArgs({ allowPositionals: true, options: {
    'trusted-build-output': { type: 'string' }, provenance: { type: 'string' }, rid: { type: 'string' }, version: { type: 'string' },
    'runtime-version': { type: 'string' }, 'runtime-channel': { type: 'string' }, 'nuget-snapshot': { type: 'string' }, output: { type: 'string' }, probe: { type: 'boolean' },
  } });
  if (positionals.length === 1 && positionals[0] === 'fetch-nuget' && values.output && path.isAbsolute(values.output)) {
    writeFileSync(values.output, JSON.stringify(await acquireNugetSnapshot(values['runtime-channel'] ?? null), null, 2) + '\n', { flag: 'wx' });
  } else if (positionals.length === 1 && values['trusted-build-output'] && values.provenance) {
    console.log(JSON.stringify(generateResourceManifest({ directory: values['trusted-build-output'], kind: positionals[0], rid: values.rid,
      version: values.version, runtimeVersion: values['runtime-version'], provenance: values.provenance, nugetSnapshot: values['nuget-snapshot'], probe: values.probe }), null, 2));
  } else throw new Error('Usage: rust-package-resource-manifest.mjs managed|go|darwin|wsl --trusted-build-output ABS --rid RID --version VERSION --runtime-version VERSION --provenance ABS [--nuget-snapshot ABS] [--probe]; or fetch-nuget --output ABS [--runtime-channel 10.0]');
}
