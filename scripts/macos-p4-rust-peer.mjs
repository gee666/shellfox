import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { closeSync, existsSync, openSync } from 'node:fs';
import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileHash, root, run } from './rust-package-common.mjs';
import { eventually } from './macos-p4-dialog.mjs';
import { listenerStopped, requestListenerStop } from './confirmed-network-outage.mjs';

const port = '42762';
const origin = `http://127.0.0.1:${port}`;
const hostName = 'shellfox-p4-host';
const viewerName = 'shellfox-p4-viewer';

export const peerEnv = {
  TS_NO_LOGS_NO_SUPPORT: 'true', TS_DEBUG_ALWAYS_USE_DERP: 'true',
  TS_AUTH_KEY: '', TS_AUTHKEY: '', TS_CLIENT_SECRET: '', TS_CLIENT_ID: '',
};
export const viewerFlags = ['--remote-control-url', origin, '--remote-hostname', viewerName];

function launch(file, args, options, log) {
  const fd = openSync(log, 'wx');
  try {
    const child = spawn(file, args, { ...options, stdio: ['ignore', fd, fd] });
    let ended = false;
    let failure;
    child.once('exit', () => { ended = true; });
    child.once('error', error => { failure = error; ended = true; });
    return {
      live() {
        if (ended) throw new Error(`Owned ${path.basename(file)} stopped; see ${log}`, { cause: failure });
      },
      async close() {
        if (ended) return;
        child.kill('SIGTERM');
        try { await eventually(`stop ${log}`, () => ended, Boolean, 5_000); }
        catch { child.kill('SIGKILL'); }
      },
    };
  } finally { closeSync(fd); }
}

export async function startRustPeer({ cli, helper, testcontrol, env, parent, viewerData }) {
  const home = path.join(parent, 'peer-home');
  const host = path.join(parent, 'peer-host');
  const a = path.join(home, 'a'), b = path.join(home, 'b');
  await Promise.all([home, host, a, b].map(dir => mkdir(dir, { mode: 0o700, recursive: true })));
  await writeFile(path.join(a, 'scope.txt'), 'Draft on peer A\n');
  await writeFile(path.join(b, 'scope.txt'), 'Original on peer B\n');
  const hostOptions = { cwd: home, env: { ...env, ...peerEnv, HOME: home, USERPROFILE: home } };
  const digest = fileHash(helper);
  const flags = (data, network) => ['--data-dir', data, '--helper', helper, '--helper-sha256', digest, ...network];
  const hostFlags = flags(host, ['--remote-control-url', origin, '--remote-hostname', hostName]);
  const invoke = (options, ...args) => JSON.parse(run(cli, [...options, ...args], { ...hostOptions, timeout: 15_000 }));
  const hostCLI = (...args) => invoke(hostFlags, ...args);
  const viewer = (...args) => invoke(flags(viewerData, viewerFlags), ...args);
  const status = cli => cli('host', 'network', 'status');
  const control = launch(testcontrol, [port], hostOptions, path.join(parent, 'peer-control.log'));
  let owner;
  let peer;
  const close = async () => {
    try {
      if (owner && existsSync(path.join(host, 'rust-service-v1/endpoint.json'))) {
        try { hostCLI('host', 'stop'); } catch (error) { console.error('Owned P4 peer cleanup:', error.message); }
      }
    } finally { await owner?.close(); await control.close(); }
  };
  try {
    await eventually('owned P4 testcontrol readiness', async () => {
      control.live();
      try {
        const response = await fetch(`${origin}/_shellfox_ci_ready`, { signal: AbortSignal.timeout(750), redirect: 'error' });
        return response.status === 204 && response.headers.get('X-Shellfox-Fixture') === 'testcontrol';
      } catch { return false; }
    }, Boolean);
    owner = launch(cli, [...hostFlags, 'host', 'serve'], hostOptions, path.join(parent, 'peer-host.log'));
    await eventually('owned P4 host endpoint', () => {
      owner.live();
      return existsSync(path.join(host, 'rust-service-v1/endpoint.json'));
    }, Boolean);
    for (const [cwd, title] of [[a, 'P4 peer A'], [b, 'P4 peer B']]) {
      hostCLI('session', 'create', '--cwd', cwd, '--profile', 'login-shell', '--title', title);
      const session = hostCLI('session', 'list').find(item => item.title === title);
      const [tab] = hostCLI('tab', 'list', session.id);
      run('python3', [path.join(root, 'scripts/rust-package-pty-smoke.py'), cli, ...hostFlags, 'attach', tab.id],
        { ...hostOptions, timeout: 60_000 });
    }
    assert.equal(hostCLI('host', 'network', 'on', '--hostname', hostName).enabled, true);
    const current = () => status(viewer).peers.find(row => row.id === peer.id);
    const connected = () => eventually('P4 peer connected without an explicit reconnect', current, row => row?.connection === 'connected');
    return {
      async connect() {
        assert.equal(viewer('host', 'network', 'on', '--hostname', viewerName).enabled, true);
        peer = await eventually('P4 host visible only on the private test tailnet', () => status(viewer).peers.find(row => row.hostname === hostName && row.online), Boolean);
        viewer('host', 'network', 'peer', peer.id, 'connect');
        const pending = await eventually('P4 viewer requests approval', () => status(hostCLI).pending, rows => rows.length === 1);
        assert.equal(pending[0].hostname, viewerName);
        hostCLI('host', 'network', 'peer', pending[0].id, 'approve');
        await connected();
      },
      async cycleListener(duringOutage) {
        await requestListenerStop(hostCLI);
        await eventually('original P4 listener helper stopped', () => status(hostCLI), listenerStopped);
        try {
          await eventually('P4 viewer noticed the host listener outage', current, row => row?.connection === 'reconnecting');
          await duringOutage();
        } finally {
          assert.equal(hostCLI('host', 'network', 'on', '--hostname', hostName).enabled, true);
        }
        await connected();
      },
      close, a, b,
    };
  } catch (error) { await close(); throw error; }
}
