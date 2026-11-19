import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { fileHash, root, run, scratchRoot } from '../../../scripts/rust-package-common.mjs';

const port = '42761';
const controlURL = `http://127.0.0.1:${port}`;
const bundleID = 'com.shellfox.mobile.iosnativev1fixture';
const build = path.join(scratchRoot, 'ios-fixture-derived');
const app = path.join(build, 'Build/Products/Release-iphonesimulator/ShellfoxMobile.app');
const evidence = path.join(scratchRoot, 'ios-native-v1-results');
const result = path.join(evidence, 'native-v1.xcresult');
const lane = path.join(scratchRoot, 'lane-apple-ios');
const data = path.join(lane, 'host');
const project = path.join(lane, 'project');
const fixture = path.join(project, 'fixture.txt');
const sharedEnv = {
  TS_NO_LOGS_NO_SUPPORT: 'true', TS_DEBUG_ALWAYS_USE_DERP: 'true',
  TS_AUTH_KEY: '', TS_AUTHKEY: '', TS_CLIENT_SECRET: '', TS_CLIENT_ID: '',
};

function child(command, args) {
  const proc = spawn(command, args, {
    cwd: root, env: { ...process.env, ...sharedEnv }, stdio: ['ignore', 'pipe', 'inherit'],
  });
  const state = { proc, output: '', finished: false, status: null };
  proc.stdout.on('data', bytes => {
    state.output = (state.output + bytes).slice(-20_000);
    if (command === 'xcodebuild') console.log(bytes.toString().trimEnd());
  });
  proc.once('exit', (code, signal) => { state.finished = true; state.status = code ?? signal; });
  proc.once('error', error => { state.finished = true; state.status = error.message; });
  return state;
}

async function until(label, ms, condition) {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    const value = await condition();
    if (value) return value;
    await delay(400);
  }
  throw new Error(`Timed out waiting for ${label}`);
}

function live(process, label) {
  if (process.finished) throw new Error(`${label} exited: ${process.status}\n${process.output}`);
}

async function stop(process) {
  if (process && !process.finished) {
    process.proc.kill('SIGTERM');
    await until('owned fixture to stop', 5_000, () => process.finished).catch(() => process.proc.kill('SIGKILL'));
  }
}

function command(executable, args, options = {}) {
  return run(executable, args, { env: sharedEnv, timeout: 30_000, ...options });
}

if (process.platform !== 'darwin' || process.env.GITHUB_ACTIONS !== 'true') {
  throw new Error('The native v1 UI fixture requires an owned macOS GitHub Actions runner.');
}
const cargoTarget = process.env.CARGO_TARGET_DIR;
assert.equal(cargoTarget, path.join(lane, 'target'));
const cli = path.join(cargoTarget, 'debug/shellfox-cli');
const helper = path.join(scratchRoot, 'dev-helper/shellfox-tailscale');
const flags = ['--data-dir', data, '--helper', helper, '--helper-sha256', fileHash(helper),
  '--remote-control-url', controlURL, '--remote-hostname', 'shellfox-ios-ci'];
const local = (...args) => JSON.parse(command(cli, [...flags, ...args], { timeout: 12_000 }));

mkdirSync(evidence, { recursive: true });
mkdirSync(data, { recursive: true, mode: 0o700 });
mkdirSync(project, { recursive: true, mode: 0o700 });
writeFileSync(fixture, 'initial fixture text\n', { flag: 'wx' });
const devices = JSON.parse(command('xcrun', ['simctl', 'list', '-j', 'devices', 'available'])).devices;
const simulator = Object.values(devices).reverse().flat().find(device => device.name.startsWith('iPad'));
assert.ok(simulator, 'Rotation requires an available iPad simulator');
const sim = (...args) => command('xcrun', ['simctl', ...args]);
let control, host, tests;
let installed = false;
try {
  if (simulator.state !== 'Booted') sim('boot', simulator.udid);
  command('xcrun', ['simctl', 'bootstatus', simulator.udid, '-b'], { timeout: 180_000 });
  control = child(path.join(lane, 'testcontrol'), [port]);
  try {
    await until('loopback Tailscale control plane', 20_000, () => {
      live(control, 'Control plane');
      return control.output.split(/\r?\n/).includes(`SHELLFOX_TESTCONTROL_READY ${controlURL}`);
    });
  } catch (error) {
    throw new Error(`Control fixture did not report its loopback listener. Output: ${JSON.stringify(control.output)}`, { cause: error });
  }
  host = child(cli, [...flags, 'host', 'serve']);
  await until('empty isolated Rust host', 30_000, () => {
    live(host, 'Rust host');
    try { return local('session', 'list').length === 0; } catch { return false; }
  });
  local('session', 'create', '--cwd', project, '--profile', 'login-shell', '--title', 'iOS native CI');
  local('host', 'network', 'on', '--hostname', 'shellfox-ios-ci');
  sim('install', simulator.udid, app);
  installed = true;
  command('xcrun', ['simctl', 'launch', simulator.udid, bundleID, '--shellfox-native-v1', '--shellfox-control-url', controlURL],
    { env: { ...sharedEnv, SIMCTL_CHILD_TS_DEBUG_ALWAYS_USE_DERP: 'true' } });
  tests = child('xcodebuild', ['-workspace', 'mobile/ios/ShellfoxMobile.xcworkspace', '-scheme', 'ShellfoxNativeV1UI',
    '-configuration', 'Release', '-sdk', 'iphonesimulator', '-destination', `platform=iOS Simulator,id=${simulator.udid}`,
    '-derivedDataPath', build, '-resultBundlePath', result, '-parallel-testing-enabled', 'NO',
    'CODE_SIGNING_ALLOWED=NO', 'test-without-building']);
  const pending = await until('the simulator pairing request', 150_000, () => {
    live(tests, 'Native v1 UI test');
    live(host, 'Rust host');
    const { pending } = local('host', 'network', 'status');
    assert.ok(pending.length <= 1, 'Multiple peers in a private test tailnet');
    return pending[0];
  });
  // Leave the waiting-for-approval UI on screen before approving the exact pending peer.
  await delay(3_000);
  local('host', 'network', 'peer', pending.id, 'approve');
  await until('the active PTY outage marker', 180_000, () => {
    live(tests, 'Native v1 UI test');
    return existsSync(path.join(project, '.ios-outage-ready'));
  });
  // Withdraw the real embedded host listener while the simulator is attached.
  // The local host and its PTY stay running throughout the outage.
  await delay(750);
  assert.equal(local('host', 'network', 'off').enabled, false);
  await delay(12_000);
  live(tests, 'Native v1 UI test');
  assert.equal(local('host', 'network', 'on', '--hostname', 'shellfox-ios-ci').enabled, true);
  await until('native v1 UI tests to finish', 12 * 60_000, () => tests.finished);
  assert.equal(tests.status, 0, tests.output);
  assert.match(readFileSync(fixture, 'utf8'), /saved-from-ios/, 'Rust host did not commit the simulator file save');
  console.log('Native v1 simulator connected, paired, attached, echoed, resized, recovered from a real outage, reattached after backgrounding, and saved to the isolated Rust host.');
} finally {
  if (installed) {
    try { sim('io', simulator.udid, 'screenshot', path.join(evidence, 'last-screen.png')); } catch { /* Preserve the original failure. */ }
    try { sim('terminate', simulator.udid, bundleID); } catch { /* It may already be stopped. */ }
  }
  await stop(tests);
  try {
    if (host && !host.finished) {
      try { local('host', 'stop'); } finally { await stop(host); }
    }
  } finally {
    await stop(control);
  }
}
