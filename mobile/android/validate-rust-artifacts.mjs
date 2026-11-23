import { existsSync, lstatSync, readFileSync } from 'node:fs';
import { androidTargets, buildAbiFormat, buildSetFormat, defaultBuildProfile, digest, headerNames,
  selectedAndroidOutput, sha256 as hashFile } from '../../crates/shellfox-mobile/android-artifacts.mjs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const targets = Object.fromEntries(Object.entries(androidTargets).map(([abi, value]) => [abi, value.target]));
const help = 'Run node crates/shellfox-mobile/build-mobile.mjs build-android first, or pass -PshellfoxRustAndroidDir=<dir>.';
function requireValue(condition, message) { if (!condition) throw new Error(`${message}. ${help}`); }
function regular(file) {
  let stat;
  try { stat = lstatSync(file); } catch { throw new Error(`Missing artifact input ${file}. ${help}`); }
  requireValue(stat.isFile() && !stat.isSymbolicLink(), `Expected a regular artifact file: ${file}`);
  return stat;
}
function json(file) {
  requireValue(regular(file).size <= 1024 * 1024, `Oversized artifact manifest: ${file}`);
  try { return JSON.parse(readFileSync(file, 'utf8')); }
  catch { throw new Error(`Invalid artifact manifest JSON: ${file}. ${help}`); }
}
export function sha256(file) {
  regular(file);
  return hashFile(file);
}
function requiredFunctions(headersDir, jniDir) {
  const functions = new Set();
  for (const name of headerNames) {
    const text = readFileSync(join(headersDir, name), 'utf8');
    for (const match of text.matchAll(/^(?:SfmStatus|uint32_t|const char\s*\*)\s*(sfm_[a-z0-9_]+)\s*\(/gm)) functions.add(match[1]);
  }
  for (const name of ['client_jni.cpp', 'terminal_jni.cpp']) {
    const text = readFileSync(join(jniDir, name), 'utf8');
    for (const match of text.matchAll(/\b(sfm_[a-z0-9_]+)\s*\(/g)) functions.add(match[1]);
  }
  requireValue(functions.size > 0, 'No declared JNI/header functions found');
  return functions;
}

/** File/hash/declaration gate only. It does NOT inspect binary symbol tables or
 * prove link/runtime acceptance. Those remain the actual linker/runner gates. */
export function validateRustArtifacts({ artifactDir, headersDir = join(root, 'crates/shellfox-mobile/include'),
  jniDir = join(root, 'mobile/android/app/src/main/cpp'), abis = Object.keys(targets) }) {
  requireValue(Array.isArray(abis) && abis.length > 0 && abis.length <= 4
    && new Set(abis).size === abis.length && abis.every(abi => Object.hasOwn(targets, abi)), 'Invalid configured Android ABI list');
  const required = requiredFunctions(headersDir, jniDir);
  const buildOutput = existsSync(join(artifactDir, 'build-output.json'));
  const gate = json(buildOutput ? join(artifactDir, 'build-output.json') : resolve(artifactDir, '../rust-gate.json'));
  if (buildOutput) {
    requireValue(gate.format === buildSetFormat && gate.complete === true, 'Incomplete or unknown build-output format');
    requireValue(typeof gate.buildId === 'string' && /^[0-9a-f-]{36}$/.test(gate.buildId)
      && gate.builtFrom === 'live-source-build-output', 'Invalid live build-output provenance');
    requireValue(Array.isArray(gate.requiredAbis) && gate.requiredAbis.length === 4
      && new Set(gate.requiredAbis).size === 4 && gate.requiredAbis.every(abi => Object.hasOwn(targets, abi))
      && Array.isArray(gate.outputs) && gate.outputs.length === 4 && new Set(gate.outputs.map(output => output.abi)).size === 4
      && gate.outputs.every(output => Object.hasOwn(targets, output.abi)), 'Partial four-ABI build-output set');
  } else {
    requireValue(gate.format === 'shellfox-observer-v2-four-abi-compile-gate-v1', 'Missing or unsupported four-ABI acceptance manifest format');
    requireValue(digest(gate.snapshot), 'Invalid snapshot identity');
  }
  requireValue(digest(gate.sourceFingerprintBefore) && gate.sourceFingerprintBefore === gate.sourceFingerprintAfter, 'Invalid snapshot/source build provenance');
  requireValue(digest(gate.cargoLockSha256) && Array.isArray(gate.outputs), 'Missing Cargo.lock/output provenance');
  const headers = Object.fromEntries(headerNames.map(name => [name, sha256(join(headersDir, name))]));
  for (const abi of abis) {
    const directory = join(artifactDir, abi), manifestPath = join(directory, 'manifest.json');
    const manifest = json(manifestPath);
    // No migration: older manifests lack the symbol inventory the JNI check needs.
    requireValue(manifest.format === (buildOutput ? buildAbiFormat : 'shellfox-observer-v2-rust-android-gate-v1'), `Outdated or unsupported Rust artifact manifest format for ${abi}`);
    requireValue(manifest.abi === abi && manifest.target === targets[abi] && manifest.androidApi === 24, `Artifact ABI/target/API mismatch for ${abi}`);
    if (buildOutput) {
      requireValue(manifest.builtFrom === 'live-source-build-output' && manifest.buildId === gate.buildId
        && manifest.sourceFingerprintBefore === gate.sourceFingerprintBefore && manifest.sourceFingerprintAfter === gate.sourceFingerprintAfter
        && manifest.producer === 'crates/shellfox-mobile/build-mobile.mjs'
        && manifest.cargoLockSha256 === gate.cargoLockSha256, `Mixed or missing live build-output provenance for ${abi}`);
    } else {
      requireValue(manifest.builtFrom === 'immutable-source-snapshot-not-live-head'
        && manifest.snapshot === gate.snapshot && manifest.sourceFingerprint === gate.sourceFingerprintAfter
        && digest(manifest.compiledInputFingerprint) && manifest.compiledInputFingerprint === gate.compiledInputFingerprint
        && manifest.cargoLockSha256 === gate.cargoLockSha256, `Mixed or missing snapshot provenance for ${abi}`);
    }
    const profile = manifest.profile;
    requireValue(profile && Object.entries(defaultBuildProfile).every(([key, value]) => profile[key] === value), `Unsupported ABI/profile/engine declaration for ${abi}`);
    for (const name of headerNames) {
      requireValue(manifest.headers?.[name] === headers[name] && gate.headers?.[name] === headers[name]
        && sha256(join(directory, 'include', name)) === headers[name], `Source/artifact header hash mismatch: ${abi}/${name}`);
    }
    if (buildOutput) {
      requireValue(['header-declarations', 'ndk-llvm-nm'].includes(manifest.symbolInventory?.kind), `Unknown symbol inventory kind for ${abi}`);
      if (manifest.symbolInventory.kind === 'ndk-llvm-nm') requireValue(typeof manifest.symbolInventory.tool === 'string'
        && Array.isArray(manifest.symbolInventory.command), `Missing reported symbol-tool provenance for ${abi}`);
    }
    const inventory = buildOutput ? manifest.symbolInventory?.functions : manifest.exportedHeaderFunctions;
    requireValue(Array.isArray(inventory) && inventory.length <= 256
      && inventory.every(name => typeof name === 'string' && /^sfm_[a-z0-9_]+$/.test(name)), `Missing/invalid declared function inventory for ${abi}`);
    const missing = [...required].filter(name => !inventory.includes(name));
    requireValue(missing.length === 0, `Declared JNI/header functions missing for ${abi}: ${missing.join(', ')}`);
    const accepted = gate.outputs.find(output => output?.abi === abi);
    requireValue(accepted && accepted.manifestSha256 === sha256(manifestPath), `Acceptance manifest hash mismatch for ${abi}`);
    requireValue(digest(manifest.rustArchiveSha256) && accepted.archiveSha256 === manifest.rustArchiveSha256
      && sha256(join(directory, 'libshellfox_mobile.a')) === manifest.rustArchiveSha256, `Actual Rust archive hash mismatch for ${abi}`);
  }
  return { ...(buildOutput ? { buildId: gate.buildId, evidence: 'build output, NOT runtime/snapshot acceptance' } : { snapshot: gate.snapshot }),
    abis, headerHashesVerified: true, archiveHashesVerified: true,
    symbolInventory: 'declarations only; actual symbol presence/link/runtime require their separate gates' };
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const { values } = parseArgs({ options: { 'artifact-dir': { type: 'string' }, abis: { type: 'string' }, 'print-default-artifact-dir': { type: 'boolean' } } });
    if (values['print-default-artifact-dir']) console.log(selectedAndroidOutput(root));
    else {
      const artifactDir = values['artifact-dir'] ? resolve(values['artifact-dir']) : selectedAndroidOutput(root);
      console.log(JSON.stringify(validateRustArtifacts({ artifactDir, abis: values.abis?.split(',').map(value => value.trim()) }), null, 2));
    }
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
