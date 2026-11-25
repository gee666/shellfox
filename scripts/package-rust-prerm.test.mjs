// The deb prerm stops the package's hosts and UI, never terminal controllers
// or CLI calls, and only when the package goes away or Electron replaces it.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { copyFileSync, mkdtempSync, rmSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const prerm = fileURLToPath(new URL('../resources/linux/shellfox-prerm', import.meta.url));
const linux = process.platform === 'linux' && spawnSync('dpkg', ['--version']).status === 0;

/** Fake package processes: copies of bash named like the packaged binaries. */
function fixture() {
  const lib = mkdtempSync(path.join(os.tmpdir(), 'shellfox-prerm-'));
  for (const name of ['shellfox', 'shellfox-cli']) copyFileSync('/bin/bash', path.join(lib, name));
  const start = (name, args) => spawn(path.join(lib, name), ['-c', 'sleep 20; :', ...args], { stdio: 'ignore' });
  const processes = {
    ui: start('shellfox', []),
    desktopHost: start('shellfox-cli', ['--desktop-host']),
    headlessHost: start('shellfox-cli', ['--user-data', '/x', 'host', 'run']),
    controller: start('shellfox-cli', ['--shellfox-linux-pty-v1', 'host', 'run']),
    command: start('shellfox-cli', ['terminals', 'list']),
  };
  const alive = child => child.exitCode === null && child.signalCode === null;
  const run = (...args) => spawnSync('sh', [prerm, ...args], { env: { ...process.env, SHELLFOX_PACKAGE_LIB: lib }, encoding: 'utf8' });
  const end = () => {
    for (const child of Object.values(processes)) child.kill('SIGKILL');
    rmSync(lib, { recursive: true, force: true });
  };
  return { processes, alive, run, end };
}
const settle = () => new Promise(resolve => setTimeout(resolve, 300));

test('removal and a move to Electron stop hosts and the UI only', { skip: !linux }, async () => {
  for (const args of [['remove'], ['upgrade', '0.9.15']]) {
    const { processes, alive, run, end } = fixture();
    try {
      await settle();
      const result = run(...args);
      assert.equal(result.status, 0, result.stderr);
      assert.match(result.stdout, /stopped 3 running Shellfox process/);
      await settle();
      assert.ok(!alive(processes.ui) && !alive(processes.desktopHost) && !alive(processes.headlessHost), args.join(' '));
      assert.ok(alive(processes.controller) && alive(processes.command), args.join(' '));
    } finally {
      end();
    }
  }
});

test('an upgrade to another Rust version leaves everything running', { skip: !linux }, async () => {
  const { processes, alive, run, end } = fixture();
  try {
    await settle();
    for (const args of [['upgrade', '0.10.0'], ['upgrade', '0.10.0~beta.5'], ['upgrade', '0.11.2'], ['failed-upgrade', '0.9.15'], ['deconfigure']]) {
      const result = run(...args);
      assert.equal(result.status, 0, result.stderr);
      assert.equal(result.stdout, '', args.join(' '));
    }
    await settle();
    assert.ok(Object.values(processes).every(alive));
  } finally {
    end();
  }
});
