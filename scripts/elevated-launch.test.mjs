// Never send a valid launch. These checks cannot reach ShellExecute or ConPTY.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { helperEnvironment } from './dotnet-sdk.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const helper = path.join(root, 'tmp/terminal-native', `win-${process.arch}`, 'Shellfox.ElevatedTerminal.exe');
const supported = process.platform === 'win32' && existsSync(helper);
function run(args, input = '') {
  const result = spawnSync(helper, args, { input, env: { ...process.env, ...helperEnvironment() }, encoding: 'utf8', timeout: 10000, windowsHide: true, maxBuffer: 524288 });
  assert.ifError(result.error);
  assert.equal(result.stderr, '');
  return result;
}
test('probe stays byte-compatible', { skip: !supported }, () => {
  const probe = run(['probe']);
  assert.equal(probe.status, 0);
  assert.equal(probe.stdout, 'shellfox-elevated-conpty-v1\r\n');
});
test('launch rejects wrong-type and malformed payloads before any UAC request', { skip: !supported }, () => {
  // Both have an invalid cwd as a second guard against a type regression.
  for (const type of ['launch', 'launch-isolated']) {
    const result = run(['launch'], JSON.stringify({ type, cwd: 'relative', env: {}, cols: 80, rows: 24 }) + '\n');
    assert.equal(result.status, 1);
    const frames = result.stdout.trim().split(/\r?\n/).map(line => JSON.parse(line));
    assert.deepEqual(frames, [
      { type: 'error', code: 'failed', message: 'Administrator PowerShell failed at reading the launch request.' },
      { type: 'exit', exitCode: 1 },
    ]);
  }
  const closed = run(['launch']);
  assert.equal(closed.status, 1);
  const frames = closed.stdout.trim().split(/\r?\n/).map(line => JSON.parse(line));
  assert.equal(frames[0].type, 'error');
  assert.deepEqual(frames[1], { type: 'exit', exitCode: 1 });
});
test('serve requires the private channel identity, and unknown modes do nothing', { skip: !supported }, () => {
  for (const args of [['serve'], ['serve', 'not-a-pipe', '1', '1'], ['probe', 'extra'], ['launch-isolated'], ['probe-power'], ['power-guardian']]) {
    const result = run(args);
    assert.equal(result.status, 2);
    assert.equal(result.stdout, '');
  }
});
