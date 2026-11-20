import assert from 'node:assert/strict';
import { mkdir, mkdtemp, realpath, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { parseArgs } from 'node:util';
import { root, run, scratchRoot } from './rust-package-common.mjs';
import { createFiles, startContainmentProbe } from './macos-p4-fixture.mjs';
import { runElectron } from './macos-p4-electron.mjs';
import { runRust } from './macos-p4-rust.mjs';

assert.equal(process.platform, 'darwin');
assert.equal(process.env.GITHUB_ACTIONS, 'true', 'Run only on an isolated macOS CI desktop');
const { values: args } = parseArgs({ options: {
  rust: { type: 'string' }, cli: { type: 'string' }, helper: { type: 'string' }, electron: { type: 'string' },
  'electron-project': { type: 'string' }, artifacts: { type: 'string' }, probe: { type: 'string' },
  testcontrol: { type: 'string' }, 'electron-remote-helper': { type: 'string' },
} });
for (const key of ['rust', 'cli', 'helper', 'electron', 'electron-project', 'artifacts', 'probe', 'testcontrol', 'electron-remote-helper']) {
  assert.ok(args[key], `Missing --${key}`);
}
const lane = await realpath(path.join(scratchRoot, 'lane-apple/p4'));
assert.equal(await realpath(args.artifacts), lane);
assert.equal(await realpath(args.probe), path.join(lane, 'window-probe'));
assert.equal(await realpath(path.join(lane, 'preflight.png')), path.join(lane, 'preflight.png'));
const [rust, cli, helper, electron, testcontrol, remoteHelper] = await Promise.all(
  [args.rust, args.cli, args.helper, args.electron, args.testcontrol, args['electron-remote-helper']].map(file => realpath(file)));
assert.equal(rust, path.join(scratchRoot, 'lane-apple/target/debug/shellfox'));
assert.equal(cli, path.join(scratchRoot, 'lane-apple/target/debug/shellfox-cli'));
assert.equal(testcontrol, path.join(lane, 'testcontrol'));
assert.equal(remoteHelper, path.join(lane, 'electron-test-remote-helper'));
assert.equal(helper, path.join(scratchRoot, 'dev-helper/shellfox-tailscale'));
assert.equal(electron, path.join(scratchRoot, 'build-test/main/index.cjs'));
const electronProject = await realpath(args['electron-project']);
assert.equal(electronProject, path.join(scratchRoot, 'lane-apple/p4-electron'));
const electronHelper = path.join(electronProject, 'tmp/tailscale', `darwin-${process.arch}`, 'shellfox-tailscale');
assert.equal(await realpath(electronHelper), electronHelper);
const electronBinary = await realpath(path.join(root, 'node_modules/electron/dist/Electron.app/Contents/MacOS/Electron'));
const dir = await mkdtemp(path.join(lane, 'files-'));
console.log(`P4 macOS evidence: ${dir}`);

function window(pid, executable, flag, data, action, ...values) {
  assert.ok(Number.isSafeInteger(pid) && pid > 1);
  assert.ok(data.startsWith(dir + path.sep));
  const command = run('/bin/ps', ['-ww', '-p', String(pid), '-o', 'command='], { timeout: 4_000 }).trim();
  assert.ok(command.startsWith(`${executable} `) && ` ${command} `.includes(` ${flag} ${data} `),
    `PID ${pid} is not the owned fixture: ${command}`);
  const output = run(args.probe, [String(pid), action, ...values.map(String)], { timeout: 12_000 });
  return output ? JSON.parse(output) : undefined;
}

async function environment(parent) {
  const home = path.join(parent, 'home');
  const env = { PATH: process.env.PATH, HOME: home, USERPROFILE: home, TMPDIR: path.join(parent, 'tmp'),
    XDG_CONFIG_HOME: path.join(home, 'config'), XDG_DATA_HOME: path.join(home, 'data'),
    XDG_CACHE_HOME: path.join(home, 'cache'), XDG_STATE_HOME: path.join(home, 'state'),
    TS_NO_LOGS_NO_SUPPORT: 'true' };
  await Promise.all(Object.values(env).slice(1, -1).map(folder => mkdir(folder, { recursive: true, mode: 0o700 })));
  return env;
}

const probe = await startContainmentProbe();
const failed = [];
const setup = async name => {
  const target = path.join(dir, name);
  const env = await environment(target);
  const project = await createFiles(target, probe.url);
  return { dir: target, env, project, window, async checkNetwork(label) {
    await delay(1_000);
    assert.deepEqual(probe.requests, [], `${label} contacted the loopback server`);
  } };
};
let scale;
try {
  try { scale = await runRust({ ...await setup('rust'), binary: rust, cli, helper, testcontrol }); }
  catch (error) { failed.push(error); console.error(error); }
  await writeFile(path.join(dir, 'rust-loopback.json'), JSON.stringify(probe.requests.splice(0)));
  try { await runElectron({ ...await setup('electron'), binary: electronBinary, main: electron, projectRoot: electronProject, remoteHelper, scale }); }
  catch (error) { failed.push(error); console.error(error); }
  await writeFile(path.join(dir, 'electron-loopback.json'), JSON.stringify(probe.requests));
} finally {
  await probe.close();
}
if (failed.length) throw new AggregateError(failed, `Mac Files runtime failed; evidence: ${dir}`);
console.log('Both macOS apps passed the paired Files, edit/save and native preview checks.');
