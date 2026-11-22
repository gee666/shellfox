// Headless archive: shellfox-headless-<version>-<platform>-<arch>.{zip|tar.gz}.
// Built from the same packaged shellfox-cli as the desktop installers.
//
// Transition from 0.9.x Node headless: the 0.9.x `shellfox update` accepts only
// archives that pass src/cli/headless-installation.ts, and a 0.9.x anchor
// re-checks the selected version on every launch, then runs
// `<version>/bin/node <version>/app/main.cjs ARGS`. So every archive carries:
//   bin/node(.exe)  a copy of shellfox-cli; it drops the app/main.cjs argument
//   runtime.json    the exact 0.9.x manifest
//   COMPAT_FILES    non-empty placeholders, never executed
// Remove these once 0.9.x anchors no longer need to be supported.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { fileHash, readJson, root, run, scratchRoot } from './rust-package-common.mjs';
import { packageNotices } from './package-rust-notices.mjs';

const platform = process.platform, arch = process.arch, win = platform === 'win32';
const exe = name => win ? `${name}.exe` : name;
export const headlessRoot = `shellfox-headless-${platform}-${arch}`;
export const headlessAsset = version => `shellfox-headless-${version}-${platform}-${arch}.${win ? 'zip' : 'tar.gz'}`;

const COMPAT_FILES = {
  common: ['app/main.cjs', 'service/install.mjs', 'app/node_modules/better-sqlite3/build/Release/better_sqlite3.node',
    'app/node_modules/node-pty/build/Release/pty.node'],
  win32: ['util/pagent.exe', 'app/node_modules/node-pty/build/Release/conpty.node',
    'app/node_modules/node-pty/build/Release/conpty/conpty.dll', 'app/node_modules/node-pty/build/Release/conpty/OpenConsole.exe'],
  darwin: ['app/node_modules/node-pty/build/Release/spawn-helper'],
  linux: [],
};
const PLACEHOLDER = '# Shellfox 0.9 update compatibility placeholder. Not used; see bin/shellfox-cli.\n';

function write(directory, name, text, mode = 0o644) {
  const file = path.join(directory, name);
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, text);
  chmodSync(file, mode);
}

function launchers(directory) {
  if (win) {
    write(directory, 'shellfox.cmd', '@echo off\r\n"%~dp0bin\\shellfox-cli.exe" %*\r\nexit /b %errorlevel%\r\n');
    write(directory, 'shellfox.ps1', "$ErrorActionPreference = 'Stop'\r\n& (Join-Path $PSScriptRoot 'bin/shellfox-cli.exe') @args\r\nexit $LASTEXITCODE\r\n");
    write(directory, 'shellfox', '#!/bin/sh\nexec "$(dirname -- "$0")/bin/shellfox-cli.exe" "$@"\n', 0o755);
  } else {
    write(directory, 'shellfox', '#!/bin/sh\nset -eu\nroot=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)\nexec "$root/bin/shellfox-cli" "$@"\n', 0o755);
    write(directory, 'shellfox.cmd', '@echo off\r\necho Use ./shellfox on this system.\r\nexit /b 1\r\n');
    write(directory, 'shellfox.ps1', "& (Join-Path $PSScriptRoot 'shellfox') @args\nexit $LASTEXITCODE\n");
  }
}

// `host service install` renders these from the anchor installation.
function services(directory) {
  const unit = target => `[Unit]\nDescription=Shellfox headless host\nAfter=network.target\n\n[Service]\nType=simple\n${target === 'system' ? 'User=@USER@\n' : ''}` +
    'ExecStart="@INSTALL_ROOT@/bin/shellfox-cli" --user-data "@DATA_ROOT@" host run\n' +
    `Restart=on-failure\nRestartSec=5\nTimeoutStopSec=30\nKillMode=control-group\nUMask=0077\n\n[Install]\nWantedBy=${target === 'system' ? 'multi-user.target' : 'default.target'}\n`;
  write(directory, 'service/shellfox-user.service.in', unit('user'));
  write(directory, 'service/shellfox-system.service.in', unit('system'));
  write(directory, 'service/com.shellfox.host.plist.in', `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>com.shellfox.host</string>
<key>ProgramArguments</key><array><string>@INSTALL_ROOT@/bin/shellfox-cli</string><string>--user-data</string><string>@DATA_ROOT@</string><string>host</string><string>run</string></array>
<key>RunAtLoad</key><true/>
<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
<key>ThrottleInterval</key><integer>5</integer>
<key>ExitTimeOut</key><integer>30</integer>
<key>StandardOutPath</key><string>@DATA_ROOT@/host.stdout.log</string>
<key>StandardErrorPath</key><string>@DATA_ROOT@/host.stderr.log</string>
</dict></plist>
`);
  write(directory, 'service/run-host.ps1', `param([Parameter(Mandatory=$true)][string]$UserData)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
& (Join-Path $root 'bin\\shellfox-cli.exe') --user-data $UserData host run *>> (Join-Path $UserData 'host.log')
exit $LASTEXITCODE
`);
}

const README = `Shellfox headless

Keep this directory together. Add it to PATH rather than copying a launcher.
It contains the Shellfox CLI and host, the embedded Tailscale helper and
terminal helpers. No desktop, Node runtime or display server is needed.

  ./shellfox --user-data /absolute/private/data host run --json
  ./shellfox --user-data /absolute/private/data host status --json
  ./shellfox --user-data /absolute/private/data host stop --yes --json
  ./shellfox host service install      (systemd --user, LaunchAgent or a login task)
  ./shellfox update --check

Run as a normal user, not root or Administrator.
Guide: https://github.com/gee666/shellfox/blob/main/docs/headless.md
bin/node, app/ and the files under app/node_modules and util/ only let
Shellfox 0.9 installations update to this version. They are not used.
`;

export function packageHeadless({ profile, stage, artifacts, version, target, sign }) {
  const work = mkdtempSync(path.join(scratchRoot, 'headless-'));
  try {
    const directory = path.join(work, headlessRoot);
    const cli = path.join(profile, exe('shellfox-cli'));
    for (const name of ['shellfox-cli', 'node']) {
      mkdirSync(path.join(directory, 'bin'), { recursive: true });
      copyFileSync(cli, path.join(directory, 'bin', exe(name)));
      chmodSync(path.join(directory, 'bin', exe(name)), 0o755);
      sign?.(path.join(directory, 'bin', exe(name)));
    }
    if (win) copyFileSync(path.join(profile, 'ghostty-vt.dll'), path.join(directory, 'bin/ghostty-vt.dll'));
    const resources = path.join(directory, 'resources');
    cpSync(path.join(stage, 'resources/tailscale'), path.join(resources, 'tailscale'), { recursive: true });
    if (platform === 'darwin') cpSync(path.join(stage, 'resources/terminal-native'), path.join(resources, 'terminal-native'), { recursive: true });
    cpSync(path.join(stage, 'notices'), path.join(resources, 'notices'), { recursive: true });
    packageNotices(resources, target, ['shellfox-cli']);
    copyFileSync(path.join(root, 'LICENSE'), path.join(directory, 'LICENSE'));
    write(directory, 'build.json', JSON.stringify({ product: 'headless', version, platform, arch }, null, 2) + '\n');
    write(directory, 'runtime.json', JSON.stringify({ version: 1, node: win ? 'bin/node.exe' : 'bin/node', entry: 'app/main.cjs',
      resources: 'resources', runAsNode: false, headless: true }, null, 2) + '\n');
    for (const name of [...COMPAT_FILES.common, ...COMPAT_FILES[platform]]) write(directory, name, PLACEHOLDER, name.endsWith('spawn-helper') ? 0o755 : 0o644);
    launchers(directory);
    services(directory);
    write(directory, 'README.txt', README);
    const archive = path.join(artifacts, headlessAsset(version));
    if (win) run(path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/tar.exe'), ['-a', '-cf', archive, '-C', work, headlessRoot]);
    // macOS bsdtar must not add AppleDouble ._ files or extended attributes.
    else run('tar', ['-czf', archive, ...(platform === 'darwin' ? ['--no-mac-metadata'] : []), '-C', work, headlessRoot], { env: { COPYFILE_DISABLE: '1' } });
    writeFileSync(archive + '.sha256', `${fileHash(archive)}  ${path.basename(archive)}\n`);
    return archive;
  } finally { rmSync(work, { recursive: true, force: true }); }
}

/** Mirrors src/cli/headless-installation.ts (0.9.x) so this archive passes its updater. */
export function assertNodeUpdaterAccepts(directory, version) {
  const build = readJson(path.join(directory, 'build.json'));
  assert.deepEqual([build.product, build.version, build.platform, build.arch], ['headless', version, platform, arch]);
  assert.deepEqual(readJson(path.join(directory, 'runtime.json')),
    { version: 1, node: win ? 'bin/node.exe' : 'bin/node', entry: 'app/main.cjs', resources: 'resources', runAsNode: false, headless: true });
  const required = ['app/main.cjs', 'shellfox', 'shellfox.cmd', 'shellfox.ps1', 'service/install.mjs', `bin/node${win ? '.exe' : ''}`,
    'app/node_modules/better-sqlite3/build/Release/better_sqlite3.node', 'app/node_modules/node-pty/build/Release/pty.node',
    `resources/tailscale/shellfox-tailscale${win ? '.exe' : ''}`,
    ...(platform === 'linux' ? ['service/shellfox-user.service.in'] : []),
    ...(platform === 'darwin' ? [...COMPAT_FILES.darwin, 'resources/terminal-native/shellfox-process-snapshot', 'resources/terminal-native/shellfox-terminal-supervisor', 'service/com.shellfox.host.plist.in'] : []),
    ...(win ? ['service/run-host.ps1', ...COMPAT_FILES.win32] : [])];
  for (const name of required) assert.ok(readFileSync(path.join(directory, name)).length > 0, `0.9 updater requires ${name}`);
}

/** Extract the archive to a path with spaces and run a real host on isolated data. */
export async function smokeHeadless(archive, version) {
  const work = mkdtempSync(path.join(scratchRoot, 'headless smoke '));
  let host, stop = async () => {};
  try {
    const tar = win ? path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/tar.exe') : '/usr/bin/tar';
    run(tar, ['-xf', archive, '-C', work]);
    const directory = path.join(work, headlessRoot);
    assertNodeUpdaterAccepts(directory, version);
    const data = path.join(work, 'user data');
    const home = path.join(work, 'home');
    mkdirSync(home);
    const env = { ...process.env, HOME: home, USERPROFILE: home, XDG_CONFIG_HOME: path.join(home, 'config'), SHELLFOX_USER_DATA: data,
      APPDATA: path.join(home, 'AppData/Roaming'), LOCALAPPDATA: path.join(home, 'AppData/Local') };
    delete env.DISPLAY; delete env.WAYLAND_DISPLAY;
    for (const dir of [env.APPDATA, env.LOCALAPPDATA]) mkdirSync(dir, { recursive: true });
    const cli = path.join(directory, 'bin', exe('shellfox-cli'));
    const shellfox = (...args) => JSON.parse(run(cli, [...args, '--json'], { env, cwd: work, timeout: 30_000 }));
    assert.deepEqual(shellfox('version'), { ok: true, value: version });
    // 0.9 launchers start bin/node with the entry script first.
    const node = path.join(directory, 'bin', exe('node'));
    assert.deepEqual(JSON.parse(run(node, [path.join(directory, 'app/main.cjs'), 'version', '--json'], { env, cwd: work })), { ok: true, value: version });
    host = spawn(cli, ['--user-data', data, 'host', 'run', '--json'], { env, cwd: work, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true });
    let output = '';
    host.stdout.on('data', chunk => { output += chunk; });
    host.stderr.on('data', chunk => { output += chunk; });
    const exited = new Promise(resolve => host.once('exit', code => resolve(code)));
    stop = async () => {
      try { run(cli, ['--user-data', data, 'host', 'stop', '--yes'], { env, cwd: work, timeout: 30_000 }); } catch { /* Reported by the failing check. */ }
      await Promise.race([exited, delay(15_000)]);
    };
    for (let i = 0; i < 300 && !output.includes('"ok":true'); i++) await delay(100);
    const ready = JSON.parse(output.split('\n').find(line => line.startsWith('{')) ?? 'null');
    assert.equal(ready?.value?.mode, 'headless', `Host did not report readiness: ${output}`);
    assert.equal(shellfox('host', 'status').ok, true);
    const session = shellfox('session', 'create', work, '--title', 'Headless smoke').value;
    assert.ok(shellfox('session', 'list').value.some(item => item.id === session.id));
    assert.equal(shellfox('session', 'delete', session.id, '--yes').ok, true);
    assert.deepEqual(shellfox('host', 'stop', '--yes'), { ok: true, value: { stopped: true } });
    assert.equal(await Promise.race([exited, delay(30_000).then(() => 'timeout')]), 0);
    host = undefined;
    assert.ok(existsSync(path.join(data, 'rust-v1', 'host')));
    return 'headless archive: 0.9 updater layout, version through bin/node, host run/status/stop, session create/list/delete';
  } finally {
    if (host && host.exitCode === null) await stop();
    if (host && host.exitCode === null) host.kill();
    // Windows can keep just-run executables locked briefly; scratch is under tmp/.
    try { rmSync(work, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 }); }
    catch (error) { console.warn(`Headless smoke scratch retained: ${work} (${error.code})`); }
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { version } = readJson(path.join(root, 'package.json'));
  const directory = path.resolve(process.argv[2] ?? path.join(scratchRoot, 'rust-installers'));
  console.log(await smokeHeadless(path.join(directory, headlessAsset(version)), version));
}
