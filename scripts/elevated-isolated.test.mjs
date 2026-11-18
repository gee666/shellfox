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
test('legacy probe stays byte-compatible and isolated probe is a separate exact typed v1 schema', { skip: !supported }, () => {
  const legacy = run(['probe']);
  assert.equal(legacy.status, 0);
  assert.equal(legacy.stdout, 'shellfox-elevated-conpty-v1\r\n');
  const isolated = run(['probe-isolated']);
  assert.equal(isolated.status, 0);
  assert.deepEqual(JSON.parse(isolated.stdout), {
    protocol: 'shellfox-elevated-isolated', version: 1, shell: 'windows-powershell', noProfile: true,
  });
});
for (const mode of ['launch', 'launch-isolated']) {
  test(`${mode} rejects wrong-mode and malformed payloads before any UAC request`, { skip: !supported }, () => {
    // Both have invalid cwd/env as a second guard against a mode regression.
    for (const type of ['launch', 'launch-isolated']) {
      const result = run([mode], JSON.stringify({ type, cwd: 'relative', env: {}, cols: 80, rows: 24 }) + '\n');
      assert.equal(result.status, 1);
      const frames = result.stdout.trim().split(/\r?\n/).map(line => JSON.parse(line));
      assert.deepEqual(frames, [
        { type: 'error', code: 'failed', message: 'The elevated terminal helper could not start or disconnected.' },
        { type: 'exit', exitCode: 1 },
      ]);
    }
    const closed = run([mode]);
    assert.equal(closed.status, 1);
    const frames = closed.stdout.trim().split(/\r?\n/).map(line => JSON.parse(line));
    assert.equal(frames[0].type, 'error');
    assert.deepEqual(frames[1], { type: 'exit', exitCode: 1 });
  });
}
test('isolated server requires the original private channel identity arguments', { skip: !supported }, () => {
  for (const args of [['serve-isolated'], ['serve-isolated', 'not-a-pipe', '1', '1'], ['probe-isolated', 'extra']]) {
    const result = run(args);
    assert.equal(result.status, 2);
    assert.equal(result.stdout, '');
  }
});
