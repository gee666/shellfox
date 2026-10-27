import { copyFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { nodeVersion } from './node-runtime.mjs';

export function writeHeadlessSupport(output, runtime = { version: 1, node: process.platform === 'win32' ? 'bin/node.exe' : 'bin/node', entry: 'app/main.cjs', resources: 'resources', runAsNode: false, headless: true }) {
  writeFileSync(path.join(output, 'runtime.json'), JSON.stringify(runtime, null, 2) + '\n');
  const directory = path.join(output, 'service');
  mkdirSync(directory, { recursive: true });
  copyFileSync(fileURLToPath(new URL('./headless-service.mjs', import.meta.url)), path.join(directory, 'install.mjs'));
  writeFileSync(path.join(directory, 'run-host.ps1'), `param([Parameter(Mandatory=$true)][string]$UserData)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$env:SHELLFOX_HEADLESS = '1'
$runtime = Get-Content -Raw -LiteralPath (Join-Path $root 'runtime.json') | ConvertFrom-Json
$env:SHELLFOX_RESOURCES_PATH = [IO.Path]::GetFullPath((Join-Path $root $runtime.resources))
Remove-Item Env:ELECTRON_RUN_AS_NODE, Env:NODE_PATH, Env:NODE_OPTIONS, Env:SHELLFOX_RUNTIME_PATH, Env:SHELLFOX_PROJECT_ROOT -ErrorAction SilentlyContinue
if ($runtime.headless) { $env:SHELLFOX_RUNTIME_PATH = $root }
if ($runtime.runAsNode) { $env:ELECTRON_RUN_AS_NODE = '1' }
& (Join-Path $root $runtime.node) (Join-Path $root $runtime.entry) --user-data $UserData host run *>> (Join-Path $UserData 'host.log')
exit $LASTEXITCODE
`);
  writeFileSync(path.join(directory, 'shellfox-user.service.in'), `[Unit]
Description=Shellfox headless host
After=network.target

[Service]
Type=simple
ExecStart="@INSTALL_ROOT@/${runtime.node}" "@INSTALL_ROOT@/${runtime.entry}" --user-data "@DATA_ROOT@" host run
Environment="SHELLFOX_HEADLESS=1" "SHELLFOX_RESOURCES_PATH=@INSTALL_ROOT@/${runtime.resources}"${runtime.headless ? ' "SHELLFOX_RUNTIME_PATH=@INSTALL_ROOT@"' : ''}${runtime.runAsNode ? ' "ELECTRON_RUN_AS_NODE=1"' : ''}
Restart=on-failure
RestartSec=5
TimeoutStopSec=30
KillMode=control-group
UMask=0077

[Install]
WantedBy=default.target
`);
  writeFileSync(path.join(directory, 'shellfox-system.service.in'), `[Unit]
Description=Shellfox headless host
After=network.target

[Service]
Type=simple
User=@USER@
ExecStart="@INSTALL_ROOT@/${runtime.node}" "@INSTALL_ROOT@/${runtime.entry}" --user-data "@DATA_ROOT@" host run
Environment="SHELLFOX_HEADLESS=1" "SHELLFOX_RESOURCES_PATH=@INSTALL_ROOT@/${runtime.resources}"${runtime.headless ? ' "SHELLFOX_RUNTIME_PATH=@INSTALL_ROOT@"' : ''}${runtime.runAsNode ? ' "ELECTRON_RUN_AS_NODE=1"' : ''}
Restart=on-failure
RestartSec=5
TimeoutStopSec=30
KillMode=control-group
UMask=0077

[Install]
WantedBy=multi-user.target
`);
  writeFileSync(path.join(directory, 'com.shellfox.host.plist.in'), `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>com.shellfox.host</string>
<key>ProgramArguments</key><array><string>@INSTALL_ROOT@/${runtime.node}</string><string>@INSTALL_ROOT@/${runtime.entry}</string><string>--user-data</string><string>@DATA_ROOT@</string><string>host</string><string>run</string></array>
<key>EnvironmentVariables</key><dict><key>SHELLFOX_HEADLESS</key><string>1</string><key>SHELLFOX_RESOURCES_PATH</key><string>@INSTALL_ROOT@/${runtime.resources}</string>${runtime.headless ? '<key>SHELLFOX_RUNTIME_PATH</key><string>@INSTALL_ROOT@</string>' : ''}${runtime.runAsNode ? '<key>ELECTRON_RUN_AS_NODE</key><string>1</string>' : ''}</dict>
<key>RunAtLoad</key><true/>
<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
<key>ThrottleInterval</key><integer>5</integer>
<key>ExitTimeOut</key><integer>30</integer>
<key>StandardOutPath</key><string>@DATA_ROOT@/host.stdout.log</string>
<key>StandardErrorPath</key><string>@DATA_ROOT@/host.stderr.log</string>
</dict></plist>
`);
  if (!runtime.headless) {
    writeFileSync(path.join(output, 'README.txt'), 'Shellfox desktop CLI runtime\n\nNative dependencies and startup installer for the installed desktop CLI.\nUse shellfox host run for foreground hosting, or shellfox host service install.\nOn Windows, add --startup from an elevated shell for a limited-user S4U boot task.\nWithout --startup it is a login-only task. Neither mode is an SCM service.\nS4U boot tasks cannot use Windows integrated-network credentials or EFS files.\nThey store no password and never default to LocalSystem. Validate Task Scheduler\npolicy and batch-logon rights on the deployment machine.\nOn Linux the installer uses systemd --user; macOS uses a launchd LaunchAgent.\nUse shellfox host service uninstall --yes to remove startup gracefully.\nThis CLI shares the desktop installation. Keep the whole installation together.\nChoose the separate headless package to deploy without Electron.\n');
    return;
  }
  writeFileSync(path.join(output, 'README.txt'), `Shellfox headless

This directory is self-contained. Keep bin, app and resources together.
It includes Node ${nodeVersion}, native terminal addons, the embedded Tailscale
helper and platform terminal helpers. No Electron, Chromium or renderer.
Linux needs glibc and C/C++ runtimes compatible with the build host.
CI packages target Ubuntu 24.04, without a display. Alpine/musl is unsupported.
Windows requires Windows 10 or newer. macOS requires macOS 13 or newer.
Builds are target-native x64 or arm64, not universal binaries.

Foreground console mode
  Unix: ./shellfox --user-data /absolute/private/data host run --json
  Windows: .\\shellfox.cmd --user-data C:\\ShellfoxData host run --json
The first JSON line reports readiness. Keep the console open. Stop with
Ctrl+C, or from another console using the same --user-data directory:
  shellfox --user-data <directory> host status --json
  shellfox --user-data <directory> host stop --yes --json
Use --help for session and remote login commands. Remote login is opt-in;
starting a host does not require a Tailscale account or network access.
Give each host a private writable data directory owned by its user.
Do not run a service host and a foreground host against the same data.
Add this directory to PATH rather than copying a launcher out of it.
The PowerShell launcher is optional; shellfox.cmd needs no script policy change.
Headless administrator sessions are unavailable. No elevated terminal
helper or .NET runtime is included, and no UAC is requested. Normal host
operation needs no administrator rights.

Install same-user startup, with no login keys or passwords in task arguments:
  Unix: ./bin/node service/install.mjs install --user-data /absolute/private/data
  Windows: .\\bin\\node.exe service/install.mjs install --user-data C:\\ShellfoxData
Use uninstall instead of install to stop the host gracefully and remove startup.
Run as the intended normal user, not root or an administrator. The installer
registers systemd --user on Linux, a LaunchAgent on macOS, and a same-user
limited-privilege login Scheduled Task on Windows. It starts the host now too.
Windows task execution uses a process-only PowerShell execution-policy bypass
for the shipped runner; it does not change machine policy. Group Policy may
still prevent execution. Do not install more than one startup per data path.
Use host status to check readiness after installing.

Linux systemd
For manual or system-wide installation, templates are under service/.
Replace @INSTALL_ROOT@ and @DATA_ROOT@ with absolute paths before installing. Escape systemd specifiers such as
% as %% in paths, and quote/escape unusual characters per systemd.syntax.
For a user service, put shellfox-user.service.in, after substitution, at
~/.config/systemd/user/shellfox.service. Then run:
  systemctl --user daemon-reload
  systemctl --user enable --now shellfox
  journalctl --user -u shellfox
For a system service use shellfox-system.service.in at
/etc/systemd/system/shellfox.service, also replace @USER@ with a dedicated
unprivileged user, and create/chown the data directory for that user.
Then use sudo systemctl daemon-reload and sudo systemctl enable --now shellfox.
A user service needs login lingering if it must survive logout:
  loginctl enable-linger <user>
Stop via systemctl before replacing package files; do not modify service
policy or machine firewall rules as part of unpacking this archive.

macOS launchd
Use service/com.shellfox.host.plist.in as a per-user LaunchAgent. Replace
both path tokens, XML-escape paths, create the data directory first and
save it to ~/Library/LaunchAgents/com.shellfox.host.plist. Validate with
plutil -lint, then launchctl bootstrap gui/$(id -u) <absolute-plist-path>.
Unload with launchctl bootout gui/$(id -u) <absolute-plist-path> before
updating. Logs are in the data directory. This agent runs in the user's
login session, not as a root LaunchDaemon. Node, addons and native helpers
are ad-hoc signed. This archive has no Developer ID signature or Apple
notarization. Gatekeeper may require explicit user approval.

Windows unattended startup and SCM distinction
For unattended startup before login, open an elevated shell as the intended
normal account and run:
  .\\bin\\node.exe service/install.mjs install --user-data C:\\ShellfoxData --startup
This registers an AtStartup Scheduled Task with the current user's S4U
limited-privilege token. It starts immediately and after boot without an
interactive login or saved password. It never defaults to LocalSystem.
S4U cannot use Windows integrated-network credentials or encrypted EFS
files. Embedded Tailscale uses its own private state and HTTP/UDP traffic,
not Windows domain credentials. Validate this on your deployment machine.
Keep the package and data on local disks accessible to the intended user.
Task Scheduler policy and the account's batch-logon rights must permit it.
If you elevate using a different account, the task runs as THAT account.
For nonadmin login-only autostart, omit --startup. That mode requires the
user to stay logged in and is not a boot-time server deployment.
Both modes are Scheduled Tasks, not SCM services. node.exe is NOT an SCM
service executable. Do not use sc.exe create with node.exe or launchers.
An independently installed SCM wrapper is needed for SCM integration.
Uninstall asks the host to stop, then unregisters the task, no PID kills.
Use an elevated shell to remove a boot task. No real machine installation
or reboot test is implied by package contract tests.

Updates and removal
Stop the host, unpack a new matching OS/architecture archive into a new
directory, update your PATH/service paths and restart. Keep the private
data directory. To remove, unload/disable the service first and remove
the package directory. Removing user data is a separate, destructive step.
`);
}
