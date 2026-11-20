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
    process.stdout.write(bytes);
  });
  proc.once('close', (code, signal) => { state.finished = true; state.status = code ?? signal; });
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
const local = (...args) => JSON.parse(command(cli, [...flags, ...args]));

mkdirSync(evidence, { recursive: true });
mkdirSync(data, { recursive: true, mode: 0o700 });
mkdirSync(project, { recursive: true, mode: 0o700 });
writeFileSync(fixture, 'initial fixture text\n', { flag: 'wx' });
const devices = JSON.parse(command('xcrun', ['simctl', 'list', '-j', 'devices', 'available'])).devices;
const simulator = Object.values(devices).reverse().flat().find(device => device.name.startsWith('iPad'));
assert.ok(simulator, 'Rotation requires an available iPad simulator');
const sim = (...args) => command('xcrun', ['simctl', ...args]);
let control, host, tests;
let booted = false;
let failure;
try {
  control = child(path.join(lane, 'testcontrol'), [port]);
  await until('loopback Tailscale control plane HTTP readiness', 30_000, async () => {
    live(control, 'Control plane');
    try {
      const ready = await fetch(`${controlURL}/_shellfox_ci_ready`, { redirect: 'error', signal: AbortSignal.timeout(750) });
      return ready.status === 204 && ready.headers.get('X-Shellfox-Fixture') === 'testcontrol';
    } catch { return false; }
  });
  console.log('Starting isolated Rust host');
  host = child(cli, [...flags, 'host', 'serve']);
  const endpoint = path.join(data, 'rust-service-v1', 'endpoint.json');
  await until('the Rust host endpoint', 30_000, () => {
    live(host, 'Rust host');
    return existsSync(endpoint);
  });
  assert.deepEqual(local('session', 'list'), []);
  console.log('Creating the fixture PTY and enabling the host listener');
  local('session', 'create', '--cwd', project, '--profile', 'login-shell', '--title', 'iOS native CI');
  assert.equal(local('host', 'network', 'on', '--hostname', 'shellfox-ios-ci').enabled, true);
  console.log(`Booting ${simulator.name} (${simulator.udid})`);
  if (simulator.state !== 'Booted') sim('boot', simulator.udid);
  command('xcrun', ['simctl', 'bootstatus', simulator.udid, '-b'], { timeout: 240_000, stdio: 'inherit' });
  booted = true;
  assert.ok(existsSync(app), 'The fixture app must be built before running the UI tests');
  console.log('Starting XCTest on the owned iPad');
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
} catch (error) {
  failure = error;
  throw error;
} finally {
  if (booted) {
    try { sim('io', simulator.udid, 'screenshot', path.join(evidence, 'last-screen.png')); } catch { /* Best effort. */ }
  }
  await stop(tests);
  if (booted) {
    try { sim('terminate', simulator.udid, bundleID); } catch { /* XCTest may have stopped it. */ }
  }
  try {
    if (host && !host.finished) {
      try {
        local('host', 'stop');
        await until('graceful host exit', 10_000, () => host.finished);
        assert.equal(host.status, 0, host.output);
      } finally {
        await stop(host);
      }
    }
  } catch (error) {
    if (!failure) throw error;
    console.error('Host cleanup also failed:', error);
  } finally {
    await stop(control);
  }
}
