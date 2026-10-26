import path from 'node:path';
import { mkdir, readFile, writeFile, unlink, access } from 'node:fs/promises';
import type { CliIntegrationDto, Result } from '../../shared/contracts';
import { failure, success } from '../../shared/contracts';
import { resultSchema } from '../../shared/schemas';
import { z } from 'zod';
import { shellfoxDispatcher } from './shellfox-dispatcher';
import { runRegistry, type RegistryRunner } from './explorer';
import { helperPath, helperData, helperHostEnvironment, posixHelper as basePosixHelper } from './cli-helper';
import { customScriptContext } from './shellfox-cli-scripts';
import { launchConfig } from './shellfox-cli-launch';
import { powershellShim } from './shellfox-cli-powershell';
const posixHelper = (options: ShellfoxCliOptions, windows: boolean) => customScriptContext(basePosixHelper({ ...options, userDataIsDefault: true }, windows));
import { setWslCli, type WslCliOptions } from './wsl-cli';

export const SHELLFOX_COMMAND = 'shellfox';
const OWNER = 'Shellfox/cli-v1';
export interface CliPort { binDir: string; readonly readOnly?: true; get(): Promise<Result<CliIntegrationDto>>; set(installed: boolean): Promise<Result<CliIntegrationDto>> }
export interface ShellfoxCliOptions {
  /** Text of shellfox-update.ps1, written next to the shims (supplied by the main process bundle). */
  updateScript?: string;
  executable: string; appPath?: string; cliHelper?: string; userData?: string; binDir?: string; platform?: NodeJS.Platform;
  resourcesPath?: string; projectRoot?: string;
  registryKey?: string; run?: RegistryRunner; prefixArgs?: string[]; launchEnv?: Record<string, string>;
  /** Disabled by default with a custom bin, registry key, or registry runner. Explicitly opt in
   * only with an injected runner or an isolated guest home when testing. */
  wsl?: false | WslCliOptions;
}
const shQuote = (s: string) => "'" + s.replaceAll("'", "'\\''") + "'";
function cmdQuote(s: string): string {
  if (/["\r\n\0]/.test(s)) throw new Error('Unsafe application argument.');
  return '"' + s.replaceAll('%', '%%') + '"';
}
export function shellfoxShims(options: ShellfoxCliOptions): { cmd: string; powershell: string; posix: string; dispatcher: string; update: string } {
  const args = [...(options.appPath ? [options.appPath] : []), ...(options.prefixArgs ?? [])];
  const environment = options.launchEnv ?? {}, hostEnvironment = helperHostEnvironment(options);
  const posix = `#!/bin/sh
# ${OWNER}
unset ELECTRON_RUN_AS_NODE
export SHELLFOX_CLI_LAUNCH=${shQuote(launchConfig(options.executable, args, environment))}
shellfox_update=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)/shellfox-update.ps1 || exit 1
if [ -n "\${WSL_DISTRO_NAME:-}" ]; then
  shellfox_update=$(wslpath -w "$shellfox_update") || exit 1
elif command -v cygpath >/dev/null 2>&1; then
  shellfox_update=$(cygpath -w "$shellfox_update") || exit 1
fi
export SHELLFOX_CLI_UPDATE="$shellfox_update"
${posixHelper(options, true)}`;
  const helper = helperPath(options), runtime = path.win32.join(path.win32.dirname(helper), '../cli-runtime/node.exe'), data = helperData(options);
  const helperCall = [runtime, helper, ...(data ? ['--shellfox-default-user-data', data] : [])].map(cmdQuote).join(' ');
  const cmd = `@echo off\r\nsetlocal DisableDelayedExpansion\r\nset "ELECTRON_RUN_AS_NODE="\r\nset "SHELLFOX_CLI_DISTRO="\r\nset "SHELLFOX_CLI_WSL_CWD="\r\nset "SHELLFOX_CLI_GIT_BASH="\r\nset "SHELLFOX_CLI_GIT_CWD="\r\nset "SHELLFOX_RESOURCES_PATH="\r\nset "SHELLFOX_PROJECT_ROOT="\r\nset "SHELLFOX_RUNTIME_PATH="\r\nset "SHELLFOX_CLI_LAUNCH=${launchConfig(options.executable, args, environment)}"\r\nset "SHELLFOX_CLI_UPDATE=%~dp0shellfox-update.ps1"\r\n${Object.entries(hostEnvironment).map(([name, value]) => 'set ' + cmdQuote(name + '=' + value) + '\r\n').join('')}${helperCall} %*\r\nexit /b %errorlevel%\r\n`;
  return { cmd, powershell: powershellShim(runtime, helper, data, options.executable, args, environment, hostEnvironment), posix, dispatcher: shellfoxDispatcher(options), update: options.updateScript ?? '' };
}
const registryState = z.object({ installed: z.boolean() }).strict();
export class WindowsCliIntegration implements CliPort {
  readonly binDir: string;
  constructor(private readonly options: ShellfoxCliOptions) { this.binDir=options.binDir??path.win32.join(process.env.LOCALAPPDATA??'','Shellfox','bin'); }
  private platform(): NodeJS.Platform { return this.options.platform ?? process.platform; }
  private state(installed: boolean, reason: string | null = null): CliIntegrationDto { return { supported: this.platform() === 'win32', installed, command: SHELLFOX_COMMAND, reason }; }
  private async pathState(installed?: boolean): Promise<Result<{ installed: boolean }>> {
    const data = Buffer.from(JSON.stringify({ bin: this.binDir, installed: installed ?? null, key: this.options.registryKey ?? 'Environment' })).toString('base64');
    const script = `$ErrorActionPreference='Stop'
$p=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('${data}')) | ConvertFrom-Json
try {
  $key=[Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($p.key)
  try {
    $raw=[string]$key.GetValue('Path','',[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $kind=[Microsoft.Win32.RegistryValueKind]::ExpandString
    if ($key.GetValueNames() -contains 'Path') { $kind=$key.GetValueKind('Path') }
    function Normalize-PathEntry([string]$entry) { $entry=[Environment]::ExpandEnvironmentVariables($entry.Trim().Trim('"')); try { return [IO.Path]::GetFullPath($entry).TrimEnd('\\').ToLowerInvariant() } catch { return $entry.ToLowerInvariant() } }
    $bin=Normalize-PathEntry $p.bin
    $parts=@()
    if ($raw.Length -gt 0) { $parts=@($raw.Split(';')) }
    $present=@($parts | Where-Object { (Normalize-PathEntry $_) -eq $bin }).Count -gt 0
    if ($null -ne $p.installed) {
      $remaining=@($parts | Where-Object { (Normalize-PathEntry $_) -ne $bin })
      if ($p.installed) { $remaining+= $p.bin }
      $key.SetValue('Path',($remaining -join ';'),$kind)
      # Verify persisted state in this process, without a second PowerShell launch.
      $raw=[string]$key.GetValue('Path','',[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
      $parts=@()
      if ($raw.Length -gt 0) { $parts=@($raw.Split(';')) }
      $present=@($parts | Where-Object { (Normalize-PathEntry $_) -eq $bin }).Count -gt 0
    }
  } finally { $key.Dispose() }
  if ($null -ne $p.installed) {
    Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class ShellfoxEnvironmentBroadcast { [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)] public static extern IntPtr SendMessageTimeout(IntPtr hWnd,uint msg,UIntPtr wParam,string lParam,uint flags,uint timeout,out UIntPtr result); }'
    $result=[UIntPtr]::Zero
    [void][ShellfoxEnvironmentBroadcast]::SendMessageTimeout([IntPtr]0xffff,0x1a,[UIntPtr]::Zero,'Environment',2,1000,[ref]$result)
  }
  @{ok=$true;value=@{installed=[bool]$present}} | ConvertTo-Json -Compress
} catch { @{ok=$false;error=@{code='NATIVE_UNAVAILABLE';message='Shellfox user PATH update failed.';retryable=$true}} | ConvertTo-Json -Compress }
`;
    try {
      const result = resultSchema(registryState).safeParse(await (this.options.run ?? runRegistry)(script));
      return result.success ? result.data : failure('NATIVE_UNAVAILABLE', 'Invalid Shellfox PATH response.', true);
    } catch { return failure('NATIVE_UNAVAILABLE', 'Shellfox user PATH access failed.', true); }
  }
  private files(): { cmd: string; powershell: string; posix: string; dispatcher: string; update: string; marker: string } { return { ...shellfoxShims(this.options), marker: JSON.stringify({ owner: OWNER, executable: this.options.executable, appPath: this.options.appPath ?? null }) }; }
  async get(): Promise<Result<CliIntegrationDto>> {
    if (this.platform() !== 'win32') return success(this.state(false, 'Shellfox CLI integration requires Windows.'));
    // Missing/stale shims cannot be installed, regardless of PATH. Avoid starting
    // PowerShell just to prove an already-known negative on a normal app launch.
    const files = await this.inspectFiles(true);
    if (!files.ok || !files.value.installed) return files;
    const registry = await this.pathState(); if (!registry.ok) return registry;
    return this.inspectFiles(registry.value.installed);
  }
  private async inspectFiles(onPath: boolean): Promise<Result<CliIntegrationDto>> {
    const expected = this.files();
    let present = false;
    try { await access(this.options.executable); if (this.options.appPath) await access(this.options.appPath); present = (await readFile(path.join(this.binDir,'shellfox.cmd'),'utf8')) === expected.cmd && (await readFile(path.join(this.binDir,'shellfox.ps1'),'utf8')) === expected.powershell && (await readFile(path.join(this.binDir,'shellfox'),'utf8')) === expected.posix && (await readFile(path.join(this.binDir,'shellfox-owner.json'),'utf8')) === expected.marker && (await readFile(path.join(this.binDir,'shellfox-dispatch.ps1'),'utf8')) === expected.dispatcher && (await readFile(path.join(this.binDir,'shellfox-update.ps1'),'utf8')) === expected.update; } catch { /* Missing files mean not installed. */ }
    return success(this.state(present && onPath));
  }
  async set(installed: boolean): Promise<Result<CliIntegrationDto>> {
    if (this.platform() !== 'win32') return failure('UNSUPPORTED', 'Shellfox CLI integration requires Windows.');
    try {
      let owned = false;
      try { owned = JSON.parse(await readFile(path.join(this.binDir,'shellfox-owner.json'),'utf8')).owner === OWNER; } catch { /* No marker. */ }
      for (const file of ['shellfox.cmd','shellfox.ps1','shellfox','shellfox-owner.json','shellfox-dispatch.ps1','shellfox-update.ps1']) {
        try { await readFile(path.join(this.binDir,file)); if (!owned) return failure('AUTH_FAILED', 'Existing shellfox shims belong to another installation.'); } catch (error) { if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error; }
      }
      if (installed) {
        await access(this.options.executable); if (this.options.appPath) await access(this.options.appPath);
        const files = this.files(); await mkdir(this.binDir, { recursive: true });
        await writeFile(path.join(this.binDir,'shellfox.cmd'), files.cmd);
        await writeFile(path.join(this.binDir,'shellfox.ps1'), files.powershell);
        await writeFile(path.join(this.binDir,'shellfox'), files.posix, { mode: 0o755 });
        await writeFile(path.join(this.binDir,'shellfox-dispatch.ps1'), files.dispatcher);
        await writeFile(path.join(this.binDir,'shellfox-update.ps1'), files.update);
        await writeFile(path.join(this.binDir,'shellfox-owner.json'), files.marker);
      }
      const registry = await this.pathState(installed); if (!registry.ok) return registry;
      if (!installed && owned) for (const file of ['shellfox.cmd','shellfox.ps1','shellfox','shellfox-owner.json','shellfox-dispatch.ps1','shellfox-update.ps1']) await unlink(path.join(this.binDir,file)).catch(error => { if (error.code !== 'ENOENT') throw error; });
      const wslEnabled = this.options.wsl !== false && (this.options.wsl !== undefined ||
        !this.options.binDir && !this.options.registryKey && !this.options.run);
      const reason = wslEnabled ? await setWslCli(installed, this.binDir, this.options.wsl || {}) : null;
      // The mutation returns PATH state. Recheck files, not the registry in a
      // second PowerShell process; no state is cached across operations.
      const result = await this.inspectFiles(registry.value.installed);
      return result.ok && reason ? success({ ...result.value, reason }) : result;
    } catch { return failure('STORAGE_FAILED', 'Shellfox shims could not be updated.', true); }
  }
}
