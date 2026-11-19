// Build the Rust CLI and bundle the Electron CLI entry, then set SHELLFOX_TEST_RUST_CLI
// and SHELLFOX_TEST_ELECTRON_CLI to their absolute paths. On Windows also supply
// SHELLFOX_TEST_PERMISSIONS_HELPER (the verified repo-owned dev helper).
const assert = require('node:assert/strict');
const { execFileSync, spawn } = require('node:child_process');
const { createHash, randomUUID } = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const readline = require('node:readline');
const { test } = require('node:test');
const { stripVTControlCharacters } = require('node:util');
const pty = require('node-pty');

const rust = process.env.SHELLFOX_TEST_RUST_CLI;
const electron = process.env.SHELLFOX_TEST_ELECTRON_CLI;
const helper = process.env.SHELLFOX_TEST_PERMISSIONS_HELPER;
const windows = process.platform === 'win32';
const root = path.resolve(__dirname, '../../..');
const loopback = path.join(root, 'crates/shellfox-service/tests/ssh-loopback.cjs');
const password = 'test-fixture-password';
const passphrase = 'test-fixture-passphrase';
const env = { ...process.env, NO_COLOR: '1', SHELLFOX_LANE_M_FIXTURE: '1' };

function protect(file, kind) {
  if (windows) execFileSync(helper, ['--private-permissions', file, kind, 'protect'], { timeout: 8000 });
  else fs.chmodSync(file, kind === 'file' ? 0o600 : 0o700);
}
function localStore(base, kind) {
  fs.mkdirSync(base, { mode: 0o700 });
  if (kind === 'electron') return base;
  const endpoint = path.join(base, 'rust-service-v1'), state = path.join(endpoint, 'ssh');
  for (const folder of [base, endpoint, state]) {
    if (folder !== base) fs.mkdirSync(folder, { mode: 0o700 });
    protect(folder, 'directory');
  }
  return state;
}
async function startServer(port, publicKey) {
  const child = spawn(process.execPath, [loopback], {
    env: { ...env, SHELLFOX_TEST_SSH_INTERACTIVE: '1', SHELLFOX_TEST_SSH_PORT: String(port),
      SHELLFOX_TEST_SSH_PASSWORD: password, ...(publicKey ? { SHELLFOX_TEST_SSH_PUBLIC_KEY: publicKey } : {}) },
    stdio: ['ignore', 'pipe', 'inherit'],
  });
  const lines = readline.createInterface({ input: child.stdout });
  const listening = new Promise((resolve, reject) => {
    lines.once('line', value => resolve(Number(value)));
    child.once('error', reject);
    child.once('exit', (code) => reject(new Error(`SSH fixture exited: ${code}`)));
  });
  const deadline = setTimeout(() => child.kill(), 10000);
  const address = await listening.finally(() => { clearTimeout(deadline); lines.close(); });
  assert.ok(address > 0);
  return { port: address, close: () => new Promise(resolve => { child.once('exit', resolve); child.kill(); }) };
}
function terminal(kind, data, args) {
  const helperArgs = windows ? ['--helper', helper, '--helper-sha256', createHash('sha256').update(fs.readFileSync(helper)).digest('hex')] : [];
  const command = kind === 'electron' ? [process.execPath, electron, '--user-data', data, 'ssh', ...args]
    : [rust, '--data-dir', data, ...helperArgs, 'ssh', ...args];
  const child = pty.spawn(command[0], command.slice(1), { name: 'xterm-256color', cols: 110, rows: 30, cwd: root, env });
  let raw = '', status;
  child.onData(bytes => {
    raw += bytes;
    if (process.env.SHELLFOX_TEST_TRACE) console.error(kind, JSON.stringify(bytes));
  });
  child.onExit(({ exitCode }) => { status = exitCode; });
  async function exit() {
    const deadline = Date.now() + 14000;
    while (Date.now() < deadline) {
      // node-pty 1.1's Windows adapter can reap a process before its
      // output pipe closes and emit no public onExit event.
      const code = status ?? (windows ? child._agent.exitCode : undefined);
      if (code !== undefined) return code;
      await new Promise(resolve => setTimeout(resolve, 20));
    }
    throw new Error(`${kind} did not exit\n${text()}`);
  }
  const text = () => stripVTControlCharacters(raw).replaceAll('\r', '');
  async function sees(fragment) {
    const deadline = Date.now() + 12000;
    while (!text().match(fragment)) {
      if (status !== undefined || Date.now() > deadline) throw new Error(`${kind} did not show ${fragment}; status ${status}\n${text()}`);
      await new Promise(resolve => setTimeout(resolve, 20));
    }
  }
  return { sees, send: keys => child.write(keys), exit, text, stop: () => { if (status === undefined && (!windows || child._agent.exitCode === undefined)) child.kill(); } };
}

const skip = !rust || !electron || (windows && !helper);
test('SSH CLI trust, masked login, exit codes, changed key and encrypted key match Electron', { skip, timeout: 90000 }, async () => {
  const temp = fs.mkdtempSync(path.join(root, 'tmp/ssh-cli-'));
  let server;
  const clients = [];
  try {
    const key = path.join(temp, 'fixture-key');
    execFileSync('ssh-keygen', ['-q', '-t', 'ed25519', '-N', passphrase, '-f', key], { timeout: 8000 });
    protect(key, 'file');
    server = await startServer(0, fs.readFileSync(`${key}.pub`, 'utf8').trim());
    const port = server.port;
    const config = ['electron', 'rust'].map(kind => {
      const base = path.join(temp, kind);
      const state = localStore(base, kind);
      const profile = { id: randomUUID(), name: 'Fixture', host: '127.0.0.1', port, user: 'fixture', password: null, keyFile: null, remoteCwd: null, source: 'manual' };
      const privateKey = { ...profile, id: randomUUID(), name: 'Key', keyFile: key };
      const file = path.join(state, 'ssh-profiles.json');
      fs.writeFileSync(file, JSON.stringify({ version: 1, profiles: [profile, privateKey] }), { mode: 0o600 });
      protect(file, 'file');
      return { kind, base };
    });
    const newClient = (kind, base, args) => {
      const client = terminal(kind, base, args);
      clients.push(client);
      return client;
    };
    const fingerprints = [];
    for (const { kind, base } of config) {
      const deny = newClient(kind, base, ['-p', 'Fixture']);
      await deny.sees(/Trust this host\? \[y\/N\] /);
      deny.send('\r');
      await deny.sees(/host key was not trusted/);
      assert.equal(await deny.exit(), 1);
      assert.match(deny.text(), /host key was not trusted/);

      const login = newClient(kind, base, []);
      await login.sees(/↑↓ select {2}enter connect {2}esc cancel/);
      login.send('1');
      await login.sees(/Trust this host\? \[y\/N\] /);
      assert.match(login.text(), new RegExp(`connecting to fixture@127\\.0\\.0\\.1:${port} \\.\\.\\.`));
      fingerprints.push(login.text().match(/ssh-rsa SHA256:[A-Za-z0-9+/]+/)?.[0]);
      login.send('yes\r');
      await login.sees(/fixture@127\.0\.0\.1's password: /);
      login.send(`${password}\r`);
      await login.sees(/fixture ready/);
      login.send('echo ok\r');
      await login.sees(/\bok\b/);
      login.send('exit 7\r');
      await login.sees(/connection to Fixture closed/);
      assert.equal(await login.exit(), 7);
      assert.match(login.text(), /connection to Fixture closed \(exit 7\)/);
      assert.ok(!login.text().includes(password));

      const encrypted = newClient(kind, base, ['--profile', 'Key']);
      await encrypted.sees(/Passphrase:/);
      encrypted.send('incorrect\r');
      await encrypted.sees(/incorrect passphrase/);
      encrypted.send(`${passphrase}\r`);
      await encrypted.sees(/fixture ready/);
      encrypted.send('exit\r');
      await encrypted.sees(/connection to Key closed/);
      assert.equal(await encrypted.exit(), 0);
      assert.match(encrypted.text(), /connection to Key closed/);
      assert.ok(!encrypted.text().includes(passphrase));
    }
    assert.ok(fingerprints[0]);
    assert.equal(fingerprints[0], fingerprints[1]);
    await server.close();
    server = null;
    server = await startServer(port);
    for (const { kind, base } of config) {
      const changed = newClient(kind, base, ['Fixture']);
      await changed.sees(/host key changed for/);
      assert.equal(await changed.exit(), 1);
      assert.match(changed.text(), new RegExp(`host key changed for 127\\.0\\.0\\.1:${port}; refusing connection\\.`));
      assert.doesNotMatch(changed.text(), /Trust this host|password: /);
    }
  } finally {
    for (const client of clients) client.stop();
    if (server) await server.close();
    fs.rmSync(temp, { recursive: true, force: true });
  }
});
