import { execFile } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { promisify } from 'node:util';

const exec = promisify(execFile);
const powershell = path.win32.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/WindowsPowerShell/v1.0/powershell.exe');
type CleanupResult = 'exited' | 'not-owned' | 'terminated';

export async function cleanupHandoffFixtures(root: string, ...knownPids: Array<number | undefined>): Promise<void> {
  const pids = new Set(knownPids);
  try {
    const receipt: unknown = JSON.parse(await readFile(path.join(root, 'helper-process.json'), 'utf8'));
    if (receipt && typeof receipt === 'object') {
      if ('pid' in receipt && typeof receipt.pid === 'number') pids.add(receipt.pid);
      if ('bootstrapPid' in receipt && typeof receipt.bootstrapPid === 'number') pids.add(receipt.bootstrapPid);
    }
  } catch (error) {
    if (!(error instanceof SyntaxError) && !(error instanceof Error && 'code' in error && error.code === 'ENOENT')) throw error;
  }
  const results = await Promise.allSettled([...pids].map(pid => terminateHandoffFixture(pid, root)));
  const failures = results.filter(result => result.status === 'rejected');
  if (failures.length) throw new AggregateError(failures.map(result => result.reason), 'Fixture cleanup failed.');
}

/** Windows test fixtures only. A PID receipt is a lookup hint, never authority to kill. */
export async function terminateHandoffFixture(pid: number | undefined, root: string): Promise<CleanupResult> {
  if (!Number.isSafeInteger(pid) || !pid || pid <= 0 || pid === process.pid) return 'not-owned';
  if (process.platform !== 'win32') throw new Error('Handoff fixture cleanup requires Windows.');
  const config = Buffer.from(JSON.stringify({ pid, root: path.resolve(root), executable: powershell }), 'utf8').toString('base64');
  const script = String.raw`$ErrorActionPreference = 'Stop'
$c = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('${config}')) | ConvertFrom-Json
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class FixtureNative {
  [DllImport("kernel32.dll", SetLastError = true)]
  [return: MarshalAs(UnmanagedType.Bool)]
  public static extern bool TerminateProcess(IntPtr process, uint code);
  [DllImport("shell32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
  private static extern IntPtr CommandLineToArgvW(string command, out int count);
  [DllImport("kernel32.dll")]
  private static extern IntPtr LocalFree(IntPtr memory);
  public static string[] Arguments(string command) {
    int count;
    IntPtr memory = CommandLineToArgvW(command, out count);
    if (memory == IntPtr.Zero) throw new Win32Exception();
    try {
      var args = new string[count];
      for (int i = 0; i < count; i++) args[i] = Marshal.PtrToStringUni(Marshal.ReadIntPtr(memory, i * IntPtr.Size));
      return args;
    } finally { LocalFree(memory); }
  }
}
'@
function SamePath($a, $b) {
  return $a -and $b -and [IO.Path]::GetFullPath($a).Equals([IO.Path]::GetFullPath($b), [StringComparison]::OrdinalIgnoreCase)
}
function ArgumentValue([string[]]$argv, [string]$flag) {
  for ($i = 1; $i -lt $argv.Length - 1; $i++) {
    if ($argv[$i] -ieq $flag) { return $argv[$i + 1] }
  }
}
function OwnedHelper([string[]]$argv) {
  $file = ArgumentValue $argv '-File'
  $control = ArgumentValue $argv '-Control'
  return (SamePath $control $c.root) -and ((SamePath $file (Join-Path $c.root 'stubborn.ps1')) -or (SamePath $file (Join-Path $c.root 'benign.ps1')))
}
$target = $null
try {
  try { $target = [Diagnostics.Process]::GetProcessById([int]$c.pid) }
  catch [ArgumentException] { 'exited'; exit 0 }
  # Pin the actual process object before CIM inspection. All termination below
  # uses this handle, even if the process exits while identity is being checked.
  try { $handle = $target.Handle; $started = $target.StartTime.ToUniversalTime() }
  catch [ComponentModel.Win32Exception] { 'not-owned'; exit 0 }
  catch [InvalidOperationException] { 'exited'; exit 0 }
  $row = Get-CimInstance Win32_Process -Filter ('ProcessId = ' + $c.pid)
  if ($target.HasExited -or -not $row) { 'exited'; exit 0 }
  if (-not (SamePath $row.ExecutablePath $c.executable) -or [Math]::Abs(($row.CreationDate.ToUniversalTime() - $started).TotalMilliseconds) -gt 1) { 'not-owned'; exit 0 }
  $argv = [FixtureNative]::Arguments([string]$row.CommandLine)
  $owned = OwnedHelper $argv
  $encoded = ArgumentValue $argv '-EncodedCommand'
  if (-not $owned -and $encoded) {
    # The production bootstrap puts its fixture context in an encoded JSON
    # literal. Decode both layers instead of trusting a PID or substring match.
    $code = [Text.Encoding]::Unicode.GetString([Convert]::FromBase64String($encoded))
    foreach ($match in [regex]::Matches($code, "FromBase64String\('([A-Za-z0-9+/=]+)'\)")) {
      try {
        $context = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($match.Groups[1].Value)) | ConvertFrom-Json
        if ((SamePath $context.control $c.root) -and (SamePath $context.file $c.executable)) {
          $helperArgs = [FixtureNative]::Arguments('powershell.exe ' + $context.arguments)
          $owned = OwnedHelper $helperArgs
          if ($owned) { break }
        }
      } catch { }
    }
  }
  if (-not $owned) { 'not-owned'; exit 0 }
  if ($target.HasExited) { 'exited'; exit 0 }
  if (-not [FixtureNative]::TerminateProcess($handle, 1)) {
    if ($target.HasExited) { 'exited'; exit 0 }
    throw 'Fixture process termination failed'
  }
  if (-not $target.WaitForExit(5000)) { throw 'Fixture process did not exit' }
  'terminated'
} finally { if ($target) { $target.Dispose() } }
`;
  const result = await exec(powershell, ['-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(script, 'utf16le').toString('base64')], { timeout: 15_000, windowsHide: true, maxBuffer: 65536 });
  const status = result.stdout.trim();
  if (status !== 'exited' && status !== 'not-owned' && status !== 'terminated') throw new Error('Invalid fixture cleanup result.');
  return status;
}
