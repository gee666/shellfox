import { afterEach, test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { sha256, validateRustArtifacts } from '../validate-rust-artifacts.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const owned = [];
afterEach(() => { for (const path of owned.splice(0)) rmSync(path, { recursive: true, force: true }); });
const digest = letter => letter.repeat(64);
const profile = { defaultCreator: 'v1', legacyClientAbi: 1, observerClientAbi: 1,
  observerWireVersion: 2, observerProfile: 'observer-v2', engine: 'default-vt-1.0.1', experimentalNative002: false };
const functions = ['sfm_terminal_create', 'sfm_client_create', 'sfm_observer_v2_abi_version'];
function put(file, value) { writeFileSync(file, JSON.stringify(value)); }
function fixture() {
  mkdirSync(join(root, 'tmp'), { recursive: true });
  const directory = mkdtempSync(join(root, 'tmp/android-artifact-fixture-')); owned.push(directory);
  const artifactDir = join(directory, 'rust'), headersDir = join(directory, 'source/include'), jniDir = join(directory, 'source/jni');
  const output = join(artifactDir, 'arm64-v8a');
  for (const p of [join(output, 'include'), headersDir, jniDir]) mkdirSync(p, { recursive: true });
  const sourceHeaders = { 'shellfox_mobile.h': 'SfmStatus sfm_terminal_create(void);\n',
    'shellfox_mobile_client.h': 'SfmStatus sfm_client_create(void);\nuint32_t sfm_observer_v2_abi_version(void);\n' };
  for (const [name, content] of Object.entries(sourceHeaders)) {
    writeFileSync(join(headersDir, name), content); writeFileSync(join(output, 'include', name), content);
  }
  writeFileSync(join(jniDir, 'client_jni.cpp'), 'sfm_client_create(); sfm_observer_v2_abi_version();');
  writeFileSync(join(jniDir, 'terminal_jni.cpp'), 'sfm_terminal_create();');
  // Fixture bytes deliberately are NOT an executable/object archive. This gate
  // verifies file hashes and declarations; it must not claim binary symbols.
  writeFileSync(join(output, 'libshellfox_mobile.a'), 'fixture archive bytes, not a compiled binary');
  const headers = Object.fromEntries(Object.keys(sourceHeaders).map(name => [name, sha256(join(headersDir, name))]));
  const manifestPath = join(output, 'manifest.json'), gatePath = join(directory, 'rust-gate.json');
  const manifest = { format: 'shellfox-observer-v2-rust-android-gate-v1', snapshot: digest('a'),
    sourceFingerprint: digest('b'), compiledInputFingerprint: digest('c'), cargoLockSha256: digest('d'), headers,
    builtFrom: 'immutable-source-snapshot-not-live-head', target: 'aarch64-linux-android', abi: 'arm64-v8a', androidApi: 24,
    profile, exportedHeaderFunctions: functions, rustArchiveSha256: sha256(join(output, 'libshellfox_mobile.a')) };
  put(manifestPath, manifest);
  put(gatePath, { format: 'shellfox-observer-v2-four-abi-compile-gate-v1', snapshot: digest('a'),
    sourceFingerprintBefore: digest('b'), sourceFingerprintAfter: digest('b'), compiledInputFingerprint: digest('c'),
    cargoLockSha256: digest('d'), headers, outputs: [{ abi: 'arm64-v8a', manifestSha256: sha256(manifestPath), archiveSha256: manifest.rustArchiveSha256 }] });
  return { directory, artifactDir, headersDir, jniDir, output, manifestPath, gatePath, abis: ['arm64-v8a'] };
}
function update(f, change, accept = true) {
  const manifest = JSON.parse(readFileSync(f.manifestPath, 'utf8')); change(manifest); put(f.manifestPath, manifest);
  if (accept) { const gate = JSON.parse(readFileSync(f.gatePath, 'utf8')); gate.outputs[0].manifestSha256 = sha256(f.manifestPath); put(f.gatePath, gate); }
}

test('validates actual known manifest fields/hashes but makes no binary-symbol claim', () => {
  const f = fixture(); const result = validateRustArtifacts(f);
  assert.equal(result.snapshot, digest('a')); assert.equal(result.archiveHashesVerified, true);
  assert.equal(result.headerHashesVerified, true); assert.match(result.symbolInventory, /declarations only/);
});
test('missing observer or any JNI-required declaration fails', () => {
  const f = fixture(); update(f, m => { m.exportedHeaderFunctions = functions.filter(name => !name.includes('observer')); });
  assert.throws(() => validateRustArtifacts(f), /functions missing.*sfm_observer_v2_abi_version/);
  update(f, m => { delete m.exportedHeaderFunctions; });
  assert.throws(() => validateRustArtifacts(f), /Missing\/invalid declared function inventory/);
});
test('missing/changed staged header or changed source header fails', () => {
  const f = fixture(); writeFileSync(join(f.headersDir, 'shellfox_mobile_client.h'), 'uint32_t sfm_new_unmatched_api(void);');
  assert.throws(() => validateRustArtifacts(f), /header hash mismatch/);
  const next = fixture(); rmSync(join(next.output, 'include/shellfox_mobile.h'));
  assert.throws(() => validateRustArtifacts(next), /Missing artifact input/);
});
test('changed archive and unaccepted manifest edits fail', () => {
  const f = fixture(); writeFileSync(join(f.output, 'libshellfox_mobile.a'), 'changed');
  assert.throws(() => validateRustArtifacts(f), /Actual Rust archive hash mismatch/);
  const next = fixture(); update(next, m => { m.extra = 'not from accepted producer'; }, false);
  assert.throws(() => validateRustArtifacts(next), /Acceptance manifest hash mismatch/);
});
test('wrong ABI, target or Android API fails', () => {
  for (const [field, value] of [['abi', 'x86'], ['target', 'x86_64-linux-android'], ['androidApi', 21]]) {
    const f = fixture(); update(f, m => { m[field] = value; });
    assert.throws(() => validateRustArtifacts(f), /ABI\/target\/API mismatch/);
  }
});
test('unknown/old formats and missing manifests fail without migration/fallback', () => {
  for (const format of ['unknown-v999', 'shellfox-android-native-snapshot-v1']) {
    const f = fixture(); update(f, m => { m.format = format; });
    assert.throws(() => validateRustArtifacts(f), /Outdated or unsupported Rust artifact manifest format/);
  }
  const f = fixture(); rmSync(f.manifestPath); assert.throws(() => validateRustArtifacts(f), /Missing artifact input/);
  const next = fixture(); const gate = JSON.parse(readFileSync(next.gatePath, 'utf8')); gate.format = 'unknown'; put(next.gatePath, gate);
  assert.throws(() => validateRustArtifacts(next), /unsupported four-ABI acceptance manifest format/);
});
test('mixed/live provenance, native002 or ABI/profile mismatch fails', () => {
  for (const change of [m => { m.sourceFingerprint = digest('e'); }, m => { m.builtFrom = 'live-head'; },
    m => { m.profile = { ...m.profile, experimentalNative002: true }; }, m => { m.profile = { ...m.profile, observerClientAbi: 2 }; }]) {
    const f = fixture(); update(f, change); assert.throws(() => validateRustArtifacts(f), /provenance|ABI\/profile\/engine/);
  }
});
test('invalid configured ABI lists fail', () => {
  const f = fixture();
  for (const abis of [[], ['desktop'], ['arm64-v8a', 'arm64-v8a']]) {
    assert.throws(() => validateRustArtifacts({ ...f, abis }), /Invalid configured Android ABI list/);
  }
});
test('Gradle validation uses configured Node, selected artifact directory and precedes native tasks', () => {
  const gradle = readFileSync(join(root, 'mobile/android/app/build.gradle'), 'utf8');
  assert.match(gradle, /reactExtension = extensions\.getByType\(com\.facebook\.react\.ReactExtension\)/);
  assert.match(gradle, /reactExtension\.getNodeExecutableAndArgs\(\)\.get\(\)/);
  assert.match(gradle, /shellfoxNodeExecutable/);
  assert.match(gradle, /task\.dependsOn\(validateRustArtifacts\)/);
  assert.match(gradle, /configureCMake/); assert.match(gradle, /preBuild/);
  assert.match(gradle, /"--artifact-dir", rustAndroidDir\.get\(\)\.asFile\.absolutePath/);
  assert.match(gradle, /"-DSHELLFOX_RUST_ANDROID_DIR=" \+ rustAndroidDir\.get\(\)\.asFile\.absolutePath/);
  assert.match(gradle, /--print-default-artifact-dir/);
  assert.match(gradle, /rustAndroidDir\.finalizeValueOnRead\(\)/);
  assert.doesNotMatch(gradle, /tmp\/mobile-rust\/android|tmp\/mobile-rust-produced/);
  assert.doesNotMatch(gradle, /860d3aa1|91ed48bc/);
});
