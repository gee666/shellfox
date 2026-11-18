// Safe capability queries only. Never execute power-guardian in automated checks.
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
function probe(mode) {
  const result = spawnSync(helper, [mode], { env: { ...process.env, ...helperEnvironment() }, encoding: 'utf8', timeout: 10000, windowsHide: true, maxBuffer: 4096 });
  assert.ifError(result.error); assert.equal(result.status, 0); assert.equal(result.stderr, '');
  return result.stdout;
}
test('power capability is a separate typed v1 probe with no power/input effects', { skip: !supported }, () => {
  assert.deepEqual(JSON.parse(probe('probe-power')), {
    protocol: 'shellfox-power-guardian', version: 1, display: true, system: true,
    awayMode: true, idleMouse: true, lidRollback: true,
  });
});
test('power addition leaves both existing capabilities unchanged', { skip: !supported }, () => {
  assert.equal(probe('probe'), 'shellfox-elevated-conpty-v1\r\n');
  assert.deepEqual(JSON.parse(probe('probe-isolated')), {
    protocol: 'shellfox-elevated-isolated', version: 1, shell: 'windows-powershell', noProfile: true,
  });
});
