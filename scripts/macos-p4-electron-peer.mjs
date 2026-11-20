import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { _electron } from '@playwright/test';
import { fileHash, root } from './rust-package-common.mjs';
import { eventually } from './macos-p4-dialog.mjs';

const hostName = 'shellfox-p4-host';
const viewerName = 'shellfox-p4-viewer';
const value = result => {
  assert.equal(result?.ok, true, result?.error?.message ?? 'Electron remote operation failed');
  return result.value;
};

export async function closeElectron(app) {
  // The comparison may fail with a fixture draft left open. Suppress only
  // teardown prompts, after all close/save assertions have finished.
  for (const page of app.windows()) page.on('dialog', dialog => { void dialog.accept().catch(() => {}); });
  await Promise.race([app.evaluate(({ BrowserWindow }) => {
    for (const window of BrowserWindow.getAllWindows()) {
      window.webContents.on('will-prevent-unload', event => event.preventDefault());
    }
  }).catch(() => {}), delay(3_000, undefined, { ref: false })]);
  const closed = app.close().then(() => true);
  try {
    assert.ok(await Promise.race([closed, delay(10_000, false, { ref: false })]), 'Owned Electron did not quit');
  } catch (error) {
    if (app.process().exitCode === null) app.process().kill('SIGTERM');
    throw error;
  }
}

export async function electronRemoteEnv(parent, helper) {
  const bus = path.join(parent, 'loopback-bus');
  await mkdir(bus, { mode: 0o700 });
  return { SHELLFOX_TEST_REMOTE_HELPER: helper, SHELLFOX_TEST_REMOTE_HELPER_SHA256: fileHash(helper), SHELLFOX_TEST_REMOTE_BUS: bus };
}

/** The fixture helper supplies only loopback peer routing. Preload, approval, and Files are real. */
export async function startElectronPeer({ binary, main, env, parent, client }) {
  const profile = path.join(parent, 'peer-user-data');
  const home = path.join(parent, 'peer-home');
  const a = path.join(home, 'a'), b = path.join(home, 'b');
  await Promise.all([profile, home, a, b].map(dir => mkdir(dir, { mode: 0o700, recursive: true })));
  await writeFile(path.join(a, 'scope.txt'), 'Draft on peer A\n');
  await writeFile(path.join(b, 'scope.txt'), 'Original on peer B\n');
  const host = await _electron.launch({ executablePath: binary, cwd: root, timeout: 60_000,
    env: { ...env, HOME: home, USERPROFILE: home, SHELLFOX_TEST_APP_DATA: path.join(home, 'app-data'),
      XDG_CONFIG_HOME: path.join(home, 'config'), XDG_DATA_HOME: path.join(home, 'data'),
      XDG_CACHE_HOME: path.join(home, 'cache'), XDG_STATE_HOME: path.join(home, 'state'), SHELLFOX_TEST_VISIBLE: '0' },
    args: [main, '--test-user-data', profile, '--test-backend', 'real'],
  });
  try {
    assert.deepEqual(await host.evaluate(({ app }) => ({ pid: process.pid, executable: process.execPath, userData: app.getPath('userData') })),
      { pid: host.process().pid, executable: binary, userData: profile });
    const hostPage = await host.firstWindow({ timeout: 60_000 });
    await hostPage.waitForFunction(() => !!window.shellfoxRemote, undefined, { timeout: 20_000 });
    const state = async page => value(await page.evaluate(() => window.shellfoxRemote.getState()));
    const configure = async (page, hostname) => value(await page.evaluate(name => window.shellfoxRemote.configure({ enabled: true, hostname: name }), hostname));
    await configure(hostPage, hostName);
    await configure(client, viewerName);
    await eventually('Electron P4 peer visible', async () => (await state(client)).peers.find(peer => peer.id === hostName), Boolean);
    value(await client.evaluate(peerId => window.shellfoxRemote.connect({ peerId }), hostName));
    const pending = await eventually('Electron P4 peer requires approval', async () => (await state(hostPage)).pending, rows => rows.length === 1);
    assert.equal(pending[0].id, viewerName);
    value(await hostPage.evaluate(peerId => window.shellfoxRemote.approve({ peerId }), viewerName));
    const current = async () => (await state(client)).peers.find(peer => peer.id === hostName);
    const connected = () => eventually('Electron private peer connected without an explicit reconnect', current, row => row?.connection === 'connected');
    await connected();
    for (const [cwd, title] of [[a, 'P4 peer A'], [b, 'P4 peer B']]) {
      value(await hostPage.evaluate(input => window.shellfox.createSession(input), { cwd, title, requestId: randomUUID() }));
    }
    const close = async () => {
      await Promise.race([hostPage.evaluate(async () => {
        const snapshot = await window.shellfox.getSnapshot();
        if (!snapshot.ok) return;
        for (const session of snapshot.value.sessions) {
          for (const tab of session.tabs) {
            if (tab.generation && tab.lifecycle !== 'closed') await window.shellfox.closeTab({ tabId: tab.id, generation: tab.generation });
          }
        }
      }), delay(15_000, undefined, { ref: false })]).catch(() => {});
      await closeElectron(host);
    };
    return { a, b, close, async cycleListener(duringOutage) {
      const registration = path.join(env.SHELLFOX_TEST_REMOTE_BUS, `${hostName}.json`);
      const original = await readFile(registration, 'utf8');
      const service = JSON.parse(original);
      assert.match(service.backendUrl, /^http:\/\/127\.0\.0\.1:\d+$/);
      const deny = createServer((_, response) => response.writeHead(503).end());
      await new Promise((resolve, reject) => {
        deny.once('error', reject);
        deny.listen(0, '127.0.0.1', resolve);
      });
      const address = deny.address();
      const replace = async contents => {
        const stage = `${registration}.p4-stage`;
        await writeFile(stage, contents, { mode: 0o600 });
        await rename(stage, registration);
      };
      try {
        assert.equal(typeof address, 'object');
        await replace(JSON.stringify({ ...service, backendUrl: `http://127.0.0.1:${address.port}` }));
        await eventually('Electron viewer noticed the loopback outage', current, row => row?.connection === 'reconnecting');
        await duringOutage();
      } finally {
        try { await replace(original); }
        finally {
          deny.closeAllConnections();
          await new Promise(resolve => deny.close(resolve));
        }
      }
      await connected();
    } };
  } catch (error) { await closeElectron(host).catch(console.error); throw error; }
}
