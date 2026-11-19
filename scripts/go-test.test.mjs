import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { runGoTests } from './go-test.mjs';

const cwd = fileURLToPath(new URL('../native/mobile/', import.meta.url));
const nethealthDir = fileURLToPath(new URL('../native/tailscale/nethealth/', import.meta.url));
const packages = ['./...', 'shellfox.local/tailscale-helper/nethealth'];
const env = { CGO_ENABLED: '0' };

function recorder() {
  const calls = [];
  return { calls, spawn(command, args, options) {
    assert.equal(options.env, env);
    assert.ok(options.timeout > 0);
    calls.push([command, args, options.cwd]);
    return { status: 0, stdout: args[0] === 'list'
      ? `shellfox.local/mobile\t${cwd}\r\n\r\n${packages[1]}\t${nethealthDir}\r\n` : '' };
  } };
}

test('Windows compiles each selected package to a fixed executable and runs it in its source directory', t => {
  const scratch = fileURLToPath(new URL('../tmp/', import.meta.url));
  mkdirSync(scratch, { recursive: true });
  const directory = mkdtempSync(path.join(scratch, 'go-test-script-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const { calls, spawn } = recorder();
  const options = { go: 'go', cwd, env, packages, testTimeout: '60s', platform: 'win32', spawn, directory };
  const mobile = path.join(directory, 'mobile.test.exe');
  const nethealth = path.join(directory, 'nethealth.test.exe');
  const flags = ['-test.count=1', '-test.timeout=60s', '-test.paniconexit0'];
  runGoTests(options);
  assert.deepEqual(calls[0][1].slice(4), packages);
  assert.deepEqual(calls.slice(1), [
    ['go', ['test', '-mod=readonly', '-c', '-o', mobile, 'shellfox.local/mobile'], cwd],
    [mobile, flags, cwd],
    ['go', ['test', '-mod=readonly', '-c', '-o', nethealth, packages[1]], cwd],
    [nethealth, flags, nethealthDir],
  ]);
  const first = calls.splice(0);
  runGoTests(options);
  assert.deepEqual(calls, first);
});

test('Linux and macOS still use plain go test with the original flags', () => {
  for (const platform of ['linux', 'darwin']) {
    const { calls, spawn } = recorder();
    runGoTests({ go: 'go', cwd, env, packages, testTimeout: '60s', platform, spawn });
    runGoTests({ go: 'go', cwd, env, packages: ['./...'], platform, spawn });
    assert.deepEqual(calls, [
      ['go', ['test', '-mod=readonly', '-count=1', '-timeout=60s', ...packages], cwd],
      ['go', ['test', '-mod=readonly', '-count=1', './...'], cwd],
    ]);
  }
});
