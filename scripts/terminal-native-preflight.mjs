import assert from 'node:assert/strict';
import { accessSync, constants, lstatSync } from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { env, run, root } from './common.mjs';
import { darwinHelpers } from './terminal-native-helpers.mjs';
if (process.platform !== 'darwin' || !['x64', 'arm64'].includes(process.arch)) throw new Error('Helper preflight needs native macOS x64/arm64.');
const directory = path.resolve(process.argv[2] ?? path.join(root, 'tmp/terminal-native', 'darwin-' + process.arch));
for (const helper of darwinHelpers) {
  const file = path.join(directory, helper.basename);
  accessSync(file, constants.X_OK);
  const stat = lstatSync(file);
  assert.ok(stat.isFile(), 'Helper must be a regular executable, not a symlink.');
  assert.equal(stat.mode & 0o7777, 0o755, 'Unexpected helper executable mode.');
  const arch = spawnSync('lipo', ['-archs', file], { env, encoding: 'utf8', timeout: 10000 });
  if (arch.error) throw arch.error;
  assert.equal(arch.status, 0, arch.stderr);
  assert.equal(arch.stdout.trim(), process.arch === 'x64' ? 'x86_64' : 'arm64', 'Wrong or universal helper architecture.');
  run('codesign', ['--verify', '--strict', '--verbose=2', file], 15000);
  const probe = spawnSync(file, ['--capabilities'], {
    env: { ...env, SHELLFOX_TERMINAL_MARKER: 'shellfox-helper-preflight-v1' },
    encoding: 'utf8', timeout: 3000, maxBuffer: 16 * 1024, killSignal: 'SIGKILL',
  });
  if (probe.error) throw probe.error;
  assert.equal(probe.status, 0, probe.stderr);
  const data = JSON.parse(probe.stdout);
  assert.equal(data.platform, 'darwin');
  assert.equal(data.arch, process.arch);
  if (helper.termination) {
    assert.equal(data.version, 1);
    assert.equal(data.ownedSession, true);
    assert.equal(data.guardians, true);
  } else {
    assert.equal(data.protocol, 1);
    assert.equal(data.source, helper.source);
    assert.equal(data.kind, 'capabilities');
    assert.equal(data.ownerUid, process.getuid());
  }
  assert.equal(data.available, true, data.reason ?? 'Helper unavailable');
  assert.equal(data.reason, null);
  assert.equal(data.termination, helper.termination);
  if (!helper.termination) {
    for (const key of ['exactBirth', 'identityAccess', 'enumeration', 'argv', 'environmentMarker']) assert.equal(data[key], true, key);
  }
  console.log('Native helper architecture/signature/self-preflight passed: ' + file);
}
