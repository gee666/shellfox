import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { privateBuildPlan } from './gpui-build.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));
test('launcher output is private and never Cargo regular target', () => {
  const plan = privateBuildPlan(root, { nonce: 'owned-fixture' });
  assert.ok(plan.executable.startsWith(path.join(root, 'tmp', 'gpui-builds') + path.sep));
  assert.deepEqual(plan.args.slice(0, 6), ['rustc', '--locked', '-p', 'shellfox-desktop', '--bin', 'shellfox']);
  assert.deepEqual(plan.args.slice(-3), ['--', '-o', plan.executable]);
  assert.throws(() => privateBuildPlan(root, { nonce: '../target' }));
  assert.throws(() => privateBuildPlan('relative'));
});

test('Windows private link and explicit unit tests leave a locked regular executable alone', { skip: process.platform !== 'win32' }, () => {
  mkdirSync(path.join(root, 'tmp'), { recursive: true });
  const fixture = mkdtempSync(path.join(root, 'tmp', 'gpui-build-test-'));
  const invoke = (program, args, options = {}) => {
    const result = spawnSync(program, args, { cwd: fixture, encoding: 'utf8', timeout: 45_000, ...options, env: { ...process.env, CARGO_TARGET_DIR: path.join(fixture, 'target'), CARGO_NET_OFFLINE: 'true', ...options.env } });
    assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
    return result;
  };
  try {
    mkdirSync(path.join(fixture, 'src'));
    writeFileSync(path.join(fixture, 'Cargo.toml'), '[package]\nname="shellfox-output-check"\nversion="0.0.0"\nedition="2024"\n[workspace]\n');
    writeFileSync(path.join(fixture, 'src', 'main.rs'), 'fn main() {}\n');
    mkdirSync(path.join(fixture, 'tests'));
    writeFileSync(path.join(fixture, 'tests', 'integration.rs'), '#[test] fn binary_exists(){assert!(std::path::Path::new(env!("CARGO_BIN_EXE_shellfox-output-check")).exists());}\n');
    invoke('cargo', ['build', '--offline']);
    const regular = path.join(fixture, 'target', 'debug', 'shellfox-output-check.exe');
    const hash = () => createHash('sha256').update(readFileSync(regular)).digest('hex');
    const before = hash();
    const plan = privateBuildPlan(fixture, { packageName: 'shellfox-output-check', binName: 'shellfox-output-check', nonce: 'owned-fixture' });
    mkdirSync(plan.directory, { recursive: true });
    const script = '$guard=[IO.File]::Open($env:SHELLFOX_TEST_REGULAR,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read);try{$arguments=ConvertFrom-Json $env:SHELLFOX_TEST_ARGUMENTS;& cargo @arguments;if($LASTEXITCODE -ne 0){exit $LASTEXITCODE};& cargo test --offline --locked --bin shellfox-output-check;exit $LASTEXITCODE}finally{$guard.Dispose()}';
    // The exact production plan's output is checked. No user process or executable is opened.
    assert.equal(plan.args.at(-1), plan.executable);
    invoke('pwsh', ['-NoProfile', '-Command', script], { env: { ...process.env, SHELLFOX_TEST_REGULAR: regular, SHELLFOX_TEST_ARGUMENTS: JSON.stringify(plan.args) } });
    assert.ok(readFileSync(plan.executable).length > 0);
    assert.equal(hash(), before);
  } finally { rmSync(fixture, { recursive: true, force: true }); }
});
