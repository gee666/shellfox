import test from 'node:test';
import assert from 'node:assert/strict';
import { minimums, plan } from './build-mobile.mjs';
import { afterEach } from 'node:test';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { androidTargets, beginAndroidOutput, defaultAndroidOutputRoot, finishAndroidOutput,
  selectedAndroidOutput, stageAndroidAbi } from './android-artifacts.mjs';
import { validateRustArtifacts } from '../../mobile/android/validate-rust-artifacts.mjs';
import { root } from './build-mobile.mjs';

const owned = [];
afterEach(() => { for (const directory of owned.splice(0)) rmSync(directory, { recursive: true, force: true }); });
function fixture() {
  mkdirSync(join(root, 'tmp'), { recursive: true });
  const project = mkdtempSync(join(root, 'tmp/android-producer-fixture-')); owned.push(project);
  for (const name of ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml']) writeFileSync(join(project, name), `fixture ${name}`);
  for (const name of ['ghostty-vt-sys', 'ghostty-vt', 'shellfox-core', 'shellfox-protocol', 'shellfox-client', 'shellfox-mobile']) {
    mkdirSync(join(project, 'crates', name), { recursive: true });
    writeFileSync(join(project, 'crates', name, 'Cargo.toml'), `fixture package ${name}`);
  }
  const headersDir = join(project, 'crates/shellfox-mobile/include'), jniDir = join(project, 'jni');
  mkdirSync(headersDir); mkdirSync(jniDir);
  writeFileSync(join(headersDir, 'shellfox_mobile.h'), 'SfmStatus sfm_terminal_create(void);\n');
  writeFileSync(join(headersDir, 'shellfox_mobile_client.h'), 'SfmStatus sfm_client_create(void);\nuint32_t sfm_observer_v2_abi_version(void);\n');
  writeFileSync(join(jniDir, 'client_jni.cpp'), 'sfm_client_create(); sfm_observer_v2_abi_version();');
  writeFileSync(join(jniDir, 'terminal_jni.cpp'), 'sfm_terminal_create();');
  return { project, headersDir, jniDir };
}
function stage(run, abi, extra = {}) {
  const archive = join(run.work, androidTargets[abi].target, 'release/libshellfox_mobile.a');
  mkdirSync(join(run.work, androidTargets[abi].target, 'release'), { recursive: true });
  writeFileSync(archive, `fixture ${abi}, not native code`);
  stageAndroidAbi(run, { abi, archive, command: ['fixture-only-no-native-execution'], nativeStaticLibraries: 'fixture declarations only', ...extra });
}
function all(run) { for (const abi of Object.keys(androidTargets)) stage(run, abi); }
function read(file) { return JSON.parse(readFileSync(file, 'utf8')); }

test('publishes only a complete NEW four-ABI build-output set without acceptance claims', () => {
  const f = fixture(), run = beginAndroidOutput(f.project); all(run);
  const directory = finishAndroidOutput(run);
  assert.equal(selectedAndroidOutput(f.project), directory);
  assert.equal(defaultAndroidOutputRoot(f.project), join(f.project, 'tmp/mobile-rust-produced/android'));
  const output = read(join(directory, 'build-output.json'));
  assert.equal(output.complete, true); assert.equal(output.builtFrom, 'live-source-build-output');
  assert.equal(output.sourceFingerprintBefore, output.sourceFingerprintAfter);
  assert.equal(output.outputs.length, 4); assert.equal('snapshot' in output, false);
  const result = validateRustArtifacts({ artifactDir: directory, headersDir: f.headersDir, jniDir: f.jniDir });
  assert.equal(result.buildId, run.id); assert.match(result.evidence, /NOT runtime/);
  for (const abi of Object.keys(androidTargets)) assert.equal(read(join(directory, abi, 'manifest.json')).symbolInventory.kind, 'header-declarations');
});
test('partial and failed ABI sets never publish a root manifest/default selector', () => {
  const f = fixture(), run = beginAndroidOutput(f.project); stage(run, 'arm64-v8a');
  assert.throws(() => finishAndroidOutput(run), /All four/);
  assert.equal(existsSync(join(run.pending, 'build-output.json')), false);
  assert.throws(() => selectedAndroidOutput(f.project), /No complete Rust Android build output/);
});
test('source changes during build are refused and leave the previous complete default unchanged', () => {
  const f = fixture(), first = beginAndroidOutput(f.project); all(first); const previous = finishAndroidOutput(first);
  const previousBytes = readFileSync(join(previous, 'arm64-v8a/libshellfox_mobile.a'));
  const next = beginAndroidOutput(f.project); all(next); writeFileSync(join(f.project, 'Cargo.lock'), 'changed');
  assert.throws(() => finishAndroidOutput(next), /Mixed source/);
  assert.equal(selectedAndroidOutput(f.project), previous);
  assert.deepEqual(readFileSync(join(previous, 'arm64-v8a/libshellfox_mobile.a')), previousBytes);
});
test('mixed, tampered or unknown provenance cannot finalize', () => {
  for (const mutation of [m => { m.buildId = 'wrong'; }, m => { m.format = 'unknown'; }, m => { m.abi = 'wrong'; },
    m => { m.profile.experimentalNative002 = true; }]) {
    const f = fixture(), run = beginAndroidOutput(f.project); all(run);
    const file = join(run.pending, 'x86/manifest.json'), m = read(file); mutation(m); writeFileSync(file, JSON.stringify(m));
    assert.throws(() => finishAndroidOutput(run), /Mixed ABI/);
  }
});
test('old/outside archives and invented measurement labels are refused', () => {
  const f = fixture(), run = beginAndroidOutput(f.project), old = join(f.project, 'old.a'); writeFileSync(old, 'old fixture');
  assert.throws(() => stageAndroidAbi(run, { abi: 'arm64-v8a', archive: old }), /freshly produced/);
  assert.equal(readFileSync(old, 'utf8'), 'old fixture');
  assert.throws(() => stage(run, 'arm64-v8a', { measurement: { kind: 'pretend-measured', functions: [] } }), /Unknown symbol/);
  assert.throws(() => stage(run, 'arm64-v8a', { measurement: { kind: 'ndk-llvm-nm', functions: [] } }), /actual tool/);
});
test('a later valid build publishes a new directory and selector, never overwriting libraries', () => {
  const f = fixture(), first = beginAndroidOutput(f.project); all(first); const prior = finishAndroidOutput(first);
  const next = beginAndroidOutput(f.project); all(next); const current = finishAndroidOutput(next);
  assert.notEqual(current, prior); assert.equal(selectedAndroidOutput(f.project), current);
  assert.equal(existsSync(join(prior, 'build-output.json')), true);
  const selector = join(defaultAndroidOutputRoot(f.project), 'current.json');
  writeFileSync(selector, JSON.stringify({ format: 'unknown', directory: '../../old' }));
  assert.throws(() => selectedAndroidOutput(f.project), /invalid default/);
});
test('new build-output validator rejects partial/unknown sets without reinterpreting snapshots', () => {
  const f = fixture(), run = beginAndroidOutput(f.project); all(run); const directory = finishAndroidOutput(run);
  const file = join(directory, 'build-output.json'), original = read(file);
  for (const changed of [{ ...original, complete: false }, { ...original, outputs: original.outputs.slice(0, 3) }, { ...original, format: 'unknown' }]) {
    writeFileSync(file, JSON.stringify(changed));
    assert.throws(() => validateRustArtifacts({ artifactDir: directory, headersDir: f.headersDir, jniDir: f.jniDir }), /Incomplete|Partial/);
  }
});

test('keeps Android API24 on every supported Android architecture', () => {
  for (const target of ['aarch64-linux-android', 'armv7-linux-androideabi', 'i686-linux-android', 'x86_64-linux-android']) {
    const recipe = plan(target);
    assert.match(recipe.zigTarget, /\.24$/);
    assert.match(recipe.clang, /24-clang$/);
    assert.equal(recipe.minimum.ndk, '27.1.12297006');
  }
});
test('resolves effective installed RN minimum for all Apple targets', () => {
  const min = minimums().ios;
  for (const target of ['aarch64-apple-ios', 'aarch64-apple-ios-sim', 'x86_64-apple-ios']) {
    const recipe = plan(target);
    assert.ok(recipe.zigTarget.includes('ios.' + min));
    assert.match(recipe.sdk, /^iphone/);
  }
});
test('does not offer local host or desktop build targets', () => {
  assert.throws(() => plan('x86_64-pc-windows-msvc'), /Android or iOS/);
  assert.throws(() => plan('aarch64-apple-darwin'), /Android or iOS/);
});
