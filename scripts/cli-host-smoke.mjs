import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { root } from './common.mjs';

export async function smokeCliHost({ executable, entry, cwd, environment }) {
  const userData = await mkdtemp(path.join(root, 'tmp/cli-host-smoke-data-'));
  const invoke = args => spawnSync(executable, [entry, '--user-data', userData, ...args], { cwd, env: environment, timeout: 15000, encoding: 'utf8', windowsHide: true });
  const host = spawn(executable, [entry, '--user-data', userData, 'host', 'run', '--json'], { cwd, env: environment, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true });
  let stdout = '', stderr = '', spawnError;
  host.on('error', error => { spawnError = error; });
  host.stdout.on('data', chunk => { stdout += chunk; });
  host.stderr.on('data', chunk => { stderr += chunk; });
  const exited = new Promise(resolve => { host.once('exit', (code, signal) => resolve({ code, signal })); host.once('error', error => resolve({ error })); });
  try {
    let ready = false;
    for (let attempt = 0; attempt < 100; attempt++) {
      if (spawnError || host.exitCode !== null || host.signalCode !== null) throw new Error(`Host exited before ready: ${spawnError?.message ?? ''}\n${stdout}\n${stderr}`);
      const result = invoke(['host', 'status', '--json']);
      if (result.status === 0) {
        const status = JSON.parse(result.stdout);
        assert.equal(status.ok, true);
        assert.equal(status.value.mode, 'headless');
        assert.equal(status.value.pid, host.pid);
        assert.equal(status.value.capabilities.desktopActions, false);
        assert.equal(status.value.capabilities.administrator, false);
        ready = true; break;
      }
      await delay(100);
    }
    assert.ok(ready, `Host not ready\n${stdout}\n${stderr}`);
    await delay(0);
    assert.ok(stdout.trim().split(/\r?\n/).some(line => { try { const value = JSON.parse(line); return value.ok === true && value.value?.pid === host.pid && value.value.mode === 'headless'; } catch { return false; } }), 'Host did not print ready JSON');
    const profiles = invoke(['profiles', '--json']);
    assert.equal(profiles.status, 0, profiles.stderr || profiles.stdout);
    const profileResult = JSON.parse(profiles.stdout);
    assert.equal(profileResult.ok, true);
    assert.ok(profileResult.value.profiles.every(profile => profile.elevation !== 'administrator' || profile.available === false), 'Headless host must not expose an available administrator profile');
    const stopped = invoke(['host', 'stop', '--yes', '--json']);
    assert.equal(stopped.status, 0, stopped.stderr || stopped.stdout);
    assert.equal(JSON.parse(stopped.stdout).ok, true);
    const result = await Promise.race([exited, delay(15000).then(() => null)]);
    assert.ok(result, 'Host did not stop');
    assert.equal(result.code, 0, stderr);
    console.log('[cli-host-smoke] offline host run/status/stop passed from unrelated cwd');
  } finally {
    if (host.exitCode === null && host.signalCode === null) { host.kill('SIGTERM'); await Promise.race([exited, delay(3000)]); }
    if (host.exitCode === null && host.signalCode === null) { host.kill('SIGKILL'); await exited; }
    await rm(userData, { recursive: true, force: true });
  }
}
