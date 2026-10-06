import type { ShellfoxCliOptions } from './shellfox-cli';

/** Foreground WSL interop waits for this short-lived Windows launcher, not the GUI.
 * Folder data is a literal parameter; launch configuration is separately encoded. */
export function shellfoxDispatcher(options: ShellfoxCliOptions): string {
  const config = Buffer.from(JSON.stringify({ executable: options.executable, args: [...(options.appPath ? [options.appPath] : []), ...(options.prefixArgs ?? [])], env: options.launchEnv ?? {} }), 'utf8').toString('base64');
  return String.raw`param([Parameter(Mandatory=$true)][string]$Folder)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
try {
  $config = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('${config}')) | ConvertFrom-Json
  if ($Folder -match '[\x00-\x1f\x7f]' -or ($Folder -notmatch '^[A-Za-z]:[\\/]' -and $Folder -notmatch '^\\\\(?:wsl\.localhost|wsl\$)\\[^\\/]+\\')) { throw 'Expected a local drive or local WSL folder.' }
  if (-not [IO.Directory]::Exists($Folder)) { throw 'The directory does not exist or is inaccessible.' }
  if (-not [IO.File]::Exists($config.executable)) { throw 'The Shellfox executable is missing.' }
  if ($Folder -match '^[A-Za-z]:') {
    $drive = [IO.DriveInfo]::new($Folder.Substring(0,3))
    if ($drive.DriveType.ToString() -notin @('Fixed','Removable','Ram')) { throw 'Only local drives are supported.' }
  }
  foreach ($entry in $config.env.PSObject.Properties) { [Environment]::SetEnvironmentVariable($entry.Name,[string]$entry.Value,'Process') }
  function Quote-WindowsArgument([string]$value) {
    # CommandLineToArgvW quoting, not shell-source escaping.
    $value = [regex]::Replace($value, '(\\*)"', '$1$1\"')
    $value = [regex]::Replace($value, '(\\+)$', '$1$1')
    return '"' + $value + '"'
  }
  $arguments = @($config.args) + @('start', $Folder)
  $line = (@($arguments | ForEach-Object { Quote-WindowsArgument ([string]$_) }) -join ' ')
  # ShellExecute/Start-Process launches independently on Windows. Do not inherit
  # WSL's pipe/console through -NoNewWindow, and do not wait for GUI lifetime.
  $process = Start-Process -FilePath $config.executable -ArgumentList $line -WorkingDirectory $env:SystemRoot -PassThru
  if ($null -eq $process) { throw 'Windows did not accept the Shellfox launch.' }
  if ($process.HasExited -and $process.ExitCode -ne 0) { throw 'Shellfox rejected the launch.' }
  [Console]::WriteLine('Shellfox dispatch accepted')
  exit 0
} catch {
  [Console]::Error.WriteLine('Shellfox: dispatch failed: ' + $_.Exception.Message)
  exit 1
}
`;
}
