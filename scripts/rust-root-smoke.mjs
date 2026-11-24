// Linux as root without systemd, as on servers and in containers: the desktop
// PTY smoke, then a real host with a tab that is attached, typed into,
// and closed, plus an automation run.
// Usage: node scripts/rust-root-smoke.mjs [BUILD_DIR]   (default target/debug)
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { ensureDevelopmentHelper } from './tailscale.mjs';

assert.equal(process.platform, 'linux');
assert.equal(process.getuid(), 0, 'Run as root');
const build = path.resolve(process.argv[2] ?? 'target/debug');
const desktop = path.join(build, 'shellfox'), cli = path.join(build, 'shellfox-cli');
const helper = ensureDevelopmentHelper();
const work = mkdtempSync(path.join(os.tmpdir(), 'shellfox-root-'));
chmodSync(work, 0o700);
const home = path.join(work, 'home');
mkdirSync(home);
const env = { ...process.env, HOME: home, XDG_CONFIG_HOME: path.join(home, 'config') };
delete env.DISPLAY; delete env.WAYLAND_DISPLAY;
const flags = ['--data-dir', path.join(work, 'data'), '--helper', helper.executable, '--helper-sha256', helper.sha256];
const run = (program, args, options = {}) => {
  const result = spawnSync(program, args, { env, cwd: work, encoding: 'utf8', timeout: 60_000, ...options });
  assert.equal(result.status, 0, `${path.basename(program)} ${args.join(' ')}\n${result.stdout}\n${result.stderr}`);
  return result.stdout;
};
const shellfox = (...args) => {
  const reply = JSON.parse(run(cli, [...flags, ...args, '--json']));
  assert.equal(reply.ok, true, JSON.stringify(reply));
  return reply.value;
};
const until = async (check, what, ms = 15_000) => {
  for (const end = Date.now() + ms; !check();) {
    assert.ok(Date.now() < end, `Timed out: ${what}`);
    await delay(50);
  }
};
const alive = pid => {
  try { return !/^\d+ \(.*\) [ZX] /.test(readFileSync(`/proc/${pid}/stat`, 'utf8')); }
  catch { return false; } // Gone between the check and the read.
};
const quote = value => `'${value.replaceAll("'", "'\\''")}'`;

let host;
try {
  assert.match(run(desktop, ['--data-dir', path.join(work, 'smoke'), '--smoke']), /PASS: owned PTY/);

  host = spawn(cli, [...flags, 'host', 'run', '--json'], { env, cwd: work, stdio: ['ignore', 'pipe', 'pipe'] });
  let log = '';
  host.stdout.on('data', bytes => { log += bytes; });
  host.stderr.on('data', bytes => { log += bytes; });
  const exited = new Promise(resolve => host.once('exit', code => resolve(code)));
  await until(() => log.includes('"ok":true'), `host readiness\n${log}`);

  const session = shellfox('session', 'create', work, '--title', 'Root smoke');
  const created = shellfox('terminals', 'create', session.id);
  const tab = created.tabs.at(-1).id;

  // Attach needs a TTY, so give it one. Ctrl+] detaches; the shell stays.
  const typed = path.join(work, 'typed'), pidFile = path.join(work, 'pid');
  const attach = spawn('script', ['-qfec', [cli, ...flags, 'terminals', 'attach', tab].map(quote).join(' '), '/dev/null'],
    { env, cwd: work, stdio: ['pipe', 'pipe', 'pipe'] });
  let screen = '';
  attach.stdout.on('data', bytes => { screen += bytes; });
  const detached = new Promise(resolve => attach.once('exit', code => resolve(code)));
  await until(() => screen.includes('# '), `root prompt\n${screen}`);
  attach.stdin.write(`trap '' HUP; sleep 300 & echo $! > ${pidFile}; echo $$ > ${pidFile}.shell; echo typed > ${typed}\r`);
  await until(() => existsSync(typed) && existsSync(pidFile) && readFileSync(pidFile, 'utf8').trim(), 'typed command');
  attach.stdin.write('\x1d');
  assert.equal(await Promise.race([detached, delay(15_000).then(() => 'timeout')]), 0, screen);
  const child = Number(readFileSync(pidFile, 'utf8'));
  const shell = Number(readFileSync(`${pidFile}.shell`, 'utf8'));
  // The tab reports closed briefly while the detached lease retires.
  await until(() => shellfox('terminals', 'list').some(t => t.id === tab && t.lifecycle === 'open'), 'tab open after detach');
  assert.ok(alive(shell) && alive(child), 'Detach keeps the shell and its child');

  assert.equal(run(cli, [...flags, 'run', '--', '/bin/echo', 'root-run']), 'root-run\n');

  shellfox('terminals', 'close', tab, '--yes');
  await until(() => !alive(child), 'close sweeps the child that ignores SIGHUP');
  // Logged once, when the first terminal starts.
  assert.match(log, /Terminal containment: session /);

  shellfox('host', 'stop', '--yes');
  assert.equal(await Promise.race([exited, delay(30_000).then(() => 'timeout')]), 0, log);
  host = undefined;
  console.log('root without systemd: desktop PTY smoke, host run, attach, type, run, close and sweep passed');
} finally {
  if (host && host.exitCode === null) host.kill();
  rmSync(work, { recursive: true, force: true });
}
