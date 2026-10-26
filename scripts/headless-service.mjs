// Copied to service/install.mjs. Only Node builtins, no repository dependencies.
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

export function serviceName(userData) {
  return 'shellfox-host-' + createHash('sha256').update(path.resolve(userData)).digest('hex').slice(0, 12);
}
export const xmlEscape = value => value.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;').replaceAll("'", '&apos;');
export const systemdEscape = value => value.replaceAll('\\', '\\\\').replaceAll('"', '\\"').replaceAll('%', '%%');
export function renderSystemd(template, root, userData) {
  const paths = { INSTALL_ROOT: root, DATA_ROOT: userData };
  return template.split('\n').map(line => line.replace(/@(INSTALL_ROOT|DATA_ROOT)@/g, (_match, token) => {
    const value = systemdEscape(paths[token]);
    return line.startsWith('ExecStart=') ? value.replaceAll('$', () => '$$') : value;
  })).join('\n');
}
const psQuote = value => "'" + value.replaceAll("'", "''") + "'";
function invoke(command, args, options = {}) {
  const result = spawnSync(command, args, { timeout: 30000, stdio: 'inherit', windowsHide: true, ...options });
  if (result.error || result.status !== 0) throw new Error(`${command} failed with status ${result.status}`, { cause: result.error });
}
export function readRuntime(root) {
  const value = JSON.parse(readFileSync(path.join(root, 'runtime.json'), 'utf8'));
  if (!value || value.version !== 1 || typeof value.headless !== 'boolean' || typeof value.runAsNode !== 'boolean' || Object.keys(value).sort().join(',') !== 'entry,headless,node,resources,runAsNode,version') throw new Error('Invalid packaged runtime manifest.');
  for (const field of ['node', 'entry', 'resources']) if (typeof value[field] !== 'string' || !value[field] || path.isAbsolute(value[field]) || /[\r\n\0]/.test(value[field])) throw new Error('Invalid packaged runtime path.');
  return { ...value, node: path.resolve(root, value.node), entry: path.resolve(root, value.entry), resources: path.resolve(root, value.resources) };
}
function stopHost(root, userData) {
  const runtime = readRuntime(root);
  const node = runtime.node, main = runtime.entry;
  const env = { ...process.env, SHELLFOX_HEADLESS: '1', SHELLFOX_RESOURCES_PATH: runtime.resources };
  for (const key of ['SHELLFOX_RUNTIME_PATH', 'SHELLFOX_PROJECT_ROOT', 'ELECTRON_RUN_AS_NODE', 'NODE_OPTIONS', 'NODE_PATH']) delete env[key];
  if (runtime.headless) env.SHELLFOX_RUNTIME_PATH = root;
  if (runtime.runAsNode) env.ELECTRON_RUN_AS_NODE = '1';
  const status = spawnSync(node, [main, '--user-data', userData, 'host', 'status', '--json'], { env, timeout: 15000, stdio: 'ignore', windowsHide: true });
  if (status.error) throw status.error;
  if (status.status === 0) invoke(node, [main, '--user-data', userData, 'host', 'stop', '--yes', '--json'], { env });
}
export function installStartup({ action, userData, root, startup = false }) {
  if (!['install', 'uninstall'].includes(action)) throw new Error('Use install or uninstall.');
  if (startup && process.platform !== 'win32') throw new Error('--startup is only supported for Windows boot tasks.');
  if (!path.isAbsolute(userData) || /[\r\n\0]/.test(userData + root)) throw new Error('User data must be an absolute path without line breaks.');
  readRuntime(root);
  const name = serviceName(userData);
  const template = file => readFileSync(path.join(root, 'service', file), 'utf8');
  if (action === 'install') mkdirSync(userData, { recursive: true, mode: 0o700 });
  if (action === 'uninstall') stopHost(root, userData);
  if (process.platform === 'linux') {
    const directory = path.join(os.homedir(), '.config/systemd/user');
    const file = path.join(directory, name + '.service');
    if (action === 'install') {
      mkdirSync(directory, { recursive: true });
      writeFileSync(file, renderSystemd(template('shellfox-user.service.in'), root, userData), { mode: 0o600 });
      invoke('systemctl', ['--user', 'daemon-reload']);
      invoke('systemctl', ['--user', 'enable', '--now', name]);
    } else {
      invoke('systemctl', ['--user', 'disable', '--now', name]);
      rmSync(file, { force: true });
      invoke('systemctl', ['--user', 'daemon-reload']);
    }
  } else if (process.platform === 'darwin') {
    const directory = path.join(os.homedir(), 'Library/LaunchAgents');
    const label = 'com.shellfox.' + name;
    const file = path.join(directory, label + '.plist');
    const domain = 'gui/' + process.getuid();
    if (action === 'install') {
      mkdirSync(directory, { recursive: true });
      writeFileSync(file, template('com.shellfox.host.plist.in').replaceAll('com.shellfox.host', label).replaceAll('@INSTALL_ROOT@', () => xmlEscape(root)).replaceAll('@DATA_ROOT@', () => xmlEscape(userData)), { mode: 0o600 });
      invoke('plutil', ['-lint', file]);
      invoke('launchctl', ['bootstrap', domain, file]);
    } else {
      invoke('launchctl', ['bootout', domain, file]);
      rmSync(file, { force: true });
    }
  } else if (process.platform === 'win32') {
    const taskName = 'Shellfox-' + name;
    let script;
    if (action === 'install') {
      const runner = path.join(root, 'service/run-host.ps1');
      const argumentsText = `-NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -File "${runner}" -UserData "${userData}"`;
      script = `$ErrorActionPreference='Stop'; $user=[System.Security.Principal.WindowsIdentity]::GetCurrent().Name; ` +
        `$action=New-ScheduledTaskAction -Execute (Join-Path $env:SystemRoot 'System32\\WindowsPowerShell\\v1.0\\powershell.exe') -Argument ${psQuote(argumentsText)}; ` +
        (startup ? `$identity=[System.Security.Principal.WindowsIdentity]::GetCurrent(); if(-not ([System.Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Boot task installation requires an elevated shell of the intended user'}; $trigger=New-ScheduledTaskTrigger -AtStartup; $principal=New-ScheduledTaskPrincipal -UserId $user -LogonType S4U -RunLevel Limited; ` : `$trigger=New-ScheduledTaskTrigger -AtLogOn -User $user; $principal=New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Limited; `) +
        `$settings=New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1); ` +
        `Register-ScheduledTask -TaskName ${psQuote(taskName)} -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null; Start-ScheduledTask -TaskName ${psQuote(taskName)}`;
    } else script = `$ErrorActionPreference='Stop'; Unregister-ScheduledTask -TaskName ${psQuote(taskName)} -Confirm:$false`;
    invoke('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script]);
    console.log(startup ? 'Windows unattended boot task, S4U limited token, not an SCM service. Windows integrated-network credentials are unavailable.' : 'Windows login-autostart task, not an SCM service. Runs only while this user is logged in.');
  } else throw new Error('Unsupported OS.');
  console.log(`${action}: ${name}`);
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [action, flag, userData, ...rest] = process.argv.slice(2);
  if (flag !== '--user-data' || !userData || (rest.length && (rest.length !== 1 || rest[0] !== '--startup'))) throw new Error('Usage: bin/node service/install.mjs install|uninstall --user-data ABSOLUTE_PATH [--startup]');
  installStartup({ action, userData, startup: rest.includes('--startup'), root: path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..') });
}
