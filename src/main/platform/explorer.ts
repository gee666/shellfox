import path from 'node:path';
import { execFile } from 'node:child_process';
import type { ExplorerIntegrationDto, Result } from '../../shared/contracts';
import { failure, success } from '../../shared/contracts';
import { explorerSchema, resultSchema } from '../../shared/schemas';

export interface ExplorerPort {
  get(): Promise<Result<ExplorerIntegrationDto>>;
  set(installed: boolean): Promise<Result<ExplorerIntegrationDto>>;
}
// Same keys, marker, ownership checks and root-safe argv as native/Explorer.cs.
// The embedded release does not ship or start the retired .NET broker.
export const EXPLORER_OWNER = 'Shellfox/native-v1';
export const EXPLORER_KEYS = ['Software\\Classes\\Directory\\shell\\Shellfox', 'Software\\Classes\\Directory\\Background\\shell\\Shellfox'];
// Compatibility identifiers, used only for ownership-checked removal.
export const LEGACY_EXPLORER_OWNER = 'PiManager/native-v1';
export const LEGACY_EXPLORER_KEYS = ['Software\\Classes\\Directory\\shell\\PiManager', 'Software\\Classes\\Directory\\Background\\shell\\PiManager'];
const quoteArgument = (value: string): string => {
  if (!path.win32.isAbsolute(value) || /["\x00-\x1f]/.test(value)) throw new Error('Expected an absolute application path.');
  return '"' + value.replace(/\\+$/, '$&$&') + '"';
};
export function explorerCommand(executable: string, background: boolean, appPath?: string): string {
  if (!/\.exe$/i.test(executable)) throw new Error('Expected a Windows executable.');
  return `${quoteArgument(executable)}${appPath ? ' ' + quoteArgument(appPath) : ''} --new-session --cwd "${background ? '%V' : '%1'}\\."`;
}
export type RegistryRunner = (script: string) => Promise<unknown>;
export const runRegistry = (script: string, options: { maxBuffer?:number } = {}): Promise<unknown> => new Promise((resolve, reject) => {
  const powershell = path.win32.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32\\WindowsPowerShell\\v1.0\\powershell.exe');
  execFile(powershell, ['-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(script, 'utf16le').toString('base64')],
    { shell: false, windowsHide: true, timeout: 10000, maxBuffer: options.maxBuffer ?? 64 * 1024, encoding: 'utf8' }, (error, stdout) => {
      if (error) reject(error);
      else { try { resolve(JSON.parse(stdout.replace(/^\uFEFF/, '').trim())); } catch (parseError) { reject(parseError); } }
    });
});
export interface ExplorerOptions { executable: string; appPath?: string; platform?: NodeJS.Platform; run?: RegistryRunner; keys?: string[]; legacyKeys?: string[] }
export class WindowsExplorer implements ExplorerPort {
  constructor(private readonly options: ExplorerOptions) {}
  private async request(installed?: boolean): Promise<Result<ExplorerIntegrationDto>> {
    if ((this.options.platform ?? process.platform) !== 'win32') return installed === undefined
      ? success({ supported: false, installed: false, folderItemInstalled: false, backgroundInstalled: false, reason: 'Explorer integration requires Windows.' })
      : failure('UNSUPPORTED', 'Explorer integration requires Windows.');
    let commands: string[];
    try { commands = (this.options.keys ?? EXPLORER_KEYS).map((_, i) => explorerCommand(this.options.executable, i === 1, this.options.appPath)); }
    catch { return failure('VALIDATION', 'Explorer must launch this application with absolute paths.'); }
    // Encode data separately. Folder placeholders never become PowerShell source.
    const payload = Buffer.from(JSON.stringify({ keys: this.options.keys ?? EXPLORER_KEYS, owner: EXPLORER_OWNER, legacyKeys: this.options.legacyKeys ?? LEGACY_EXPLORER_KEYS, legacyOwner: LEGACY_EXPLORER_OWNER, commands, executable: this.options.executable, installed: installed ?? null }), 'utf8').toString('base64');
    const script = `
$ErrorActionPreference = 'Stop'
$p = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('${payload}')) | ConvertFrom-Json
try {
  # Never delete a legacy key on its name alone. Foreign verbs stay untouched.
  $removedLegacy = $false
  foreach ($name in $p.legacyKeys) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($name)
    $owned = $false
    if ($null -ne $key) { try { $owned = $key.GetValue('PiManagerOwner') -ceq $p.legacyOwner } finally { $key.Dispose() } }
    if ($owned) { [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($name, $false); $removedLegacy = $true }
  }
  if ($null -ne $p.installed) {
    if (-not [IO.File]::Exists($p.executable)) { throw 'VALIDATION' }
    if ($p.installed) {
      foreach ($name in $p.keys) {
        $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($name)
        if ($null -ne $key) { try { if ($key.GetValue('ShellfoxOwner') -cne $p.owner) { throw 'AUTH_FAILED' } } finally { $key.Dispose() } }
      }
    }
    for ($i = 0; $i -lt $p.keys.Count; $i++) {
      if ($p.installed) {
        $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($p.keys[$i])
        try {
          $key.SetValue('ShellfoxOwner', $p.owner)
          $key.SetValue('', 'Open in Shellfox')
          $key.SetValue('MUIVerb', 'Open in Shellfox')
          if ($i -eq 0) { $key.SetValue('MultiSelectModel', 'Single') }
          else { $key.DeleteValue('MultiSelectModel', $false) }
          $key.SetValue('Icon', $p.executable)
          $command = $key.CreateSubKey('command')
          try { $command.SetValue('', $p.commands[$i]) } finally { $command.Dispose() }
        } finally { $key.Dispose() }
      } else {
        $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($p.keys[$i])
        $owned = $false
        if ($null -ne $key) { try { $owned = $key.GetValue('ShellfoxOwner') -ceq $p.owner } finally { $key.Dispose() } }
        if ($owned) { [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($p.keys[$i], $false) }
      }
    }
  }
  if ($removedLegacy -or $null -ne $p.installed) {
    Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class ShellfoxExplorerNotify { [DllImport("shell32.dll")] public static extern void SHChangeNotify(uint events,uint flags,IntPtr item1,IntPtr item2); }'
    [ShellfoxExplorerNotify]::SHChangeNotify(0x08000000,0,[IntPtr]::Zero,[IntPtr]::Zero)
  }
  $flags = @()
  for ($i = 0; $i -lt $p.keys.Count; $i++) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($p.keys[$i])
    $owned = $false
    if ($null -ne $key) {
      try {
        $command = $key.OpenSubKey('command')
        if ($null -ne $command) { try { $owned = $key.GetValue('ShellfoxOwner') -ceq $p.owner -and $command.GetValue('') -ceq $p.commands[$i] -and ($i -eq 0 -or $key.GetValue('MultiSelectModel') -cne 'Single') } finally { $command.Dispose() } }
      } finally { $key.Dispose() }
    }
    $flags += [bool]$owned
  }
  @{ ok = $true; value = @{ supported = $true; installed = ($flags[0] -and $flags[1]); folderItemInstalled = $flags[0]; backgroundInstalled = $flags[1]; reason = 'Windows 11: use Show more options (Shift+F10) in Explorer.' } } | ConvertTo-Json -Compress -Depth 4
} catch {
  $code = 'NATIVE_UNAVAILABLE'
  $message = 'Explorer registry update failed.'
  if ($_.Exception.Message -eq 'AUTH_FAILED') { $code = 'AUTH_FAILED'; $message = 'An Explorer verb with this name belongs to another application. It was not changed.' }
  if ($_.Exception.Message -eq 'VALIDATION') { $code = 'VALIDATION'; $message = 'The application executable is not available.' }
  @{ ok = $false; error = @{ code = $code; message = $message; retryable = $false } } | ConvertTo-Json -Compress -Depth 4
}
`;
    try {
      const result = resultSchema(explorerSchema).safeParse(await (this.options.run ?? runRegistry)(script));
      return result.success ? result.data : failure('NATIVE_UNAVAILABLE', 'Invalid Explorer registry response.', true);
    } catch { return failure('NATIVE_UNAVAILABLE', 'Explorer registry access failed.', true); }
  }
  get(): Promise<Result<ExplorerIntegrationDto>> { return this.request(); }
  set(installed: boolean): Promise<Result<ExplorerIntegrationDto>> { return this.request(installed); }
}
