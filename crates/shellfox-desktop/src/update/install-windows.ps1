param([string]$Config)
$ErrorActionPreference = 'Stop'
$c = $null
$managerExited = $false
try {
  $c = Get-Content -LiteralPath $Config -Raw -Encoding UTF8 | ConvertFrom-Json
  if ((Get-FileHash -LiteralPath $c.file -Algorithm SHA256).Hash -ne $c.sha256) { throw 'Installer checksum changed.' }
  $ready = Join-Path $c.control 'ready'
  $commit = Join-Path $c.control 'commit'
  $cancel = Join-Path $c.control 'cancel'
  New-Item -ItemType File -Path $ready -ErrorAction Stop | Out-Null
  $deadline = [DateTime]::UtcNow.AddMinutes(10)
  while (-not (Test-Path -LiteralPath $commit)) {
    if ((Test-Path -LiteralPath $cancel) -or -not (Get-Process -Id $c.pid -ErrorAction SilentlyContinue) -or [DateTime]::UtcNow -gt $deadline) { exit 1 }
    Start-Sleep -Milliseconds 50
  }
  if (Test-Path -LiteralPath $cancel) { exit 1 }
  New-Item -ItemType File -Path (Join-Path $c.control 'committed') -ErrorAction Stop | Out-Null
  $p = Get-Process -Id $c.pid -ErrorAction SilentlyContinue
  if ($p) { Wait-Process -Id $c.pid -Timeout 120 -ErrorAction Stop }
  if (Test-Path -LiteralPath (Join-Path $c.control 'cancel')) { exit 1 }
  $managerExited = $true
  # The UI exits once its terminal host closes the connection, not when the
  # host process is gone, and a CLI may still be finishing. Setup.exe kills
  # everything under the install root, so wait for all of it to exit.
  $root = (Split-Path -Parent $c.updateExe).TrimEnd('\') + '\'
  $running = @(Get-CimInstance Win32_Process | Where-Object {
    $_.ExecutablePath -and $_.ExecutablePath.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)
  } | ForEach-Object { Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue })
  if ($running.Count) { Wait-Process -InputObject $running -Timeout 60 -ErrorAction Stop }
  $hash = (Get-FileHash -LiteralPath $c.file -Algorithm SHA256).Hash
  if ($hash -ne $c.sha256) { throw 'Installer checksum changed.' }
  $install = Start-Process -FilePath $c.file -ArgumentList '--silent' -Wait -PassThru
  if ($install.ExitCode -ne 0) { throw "Installer exited with $($install.ExitCode)" }
  if ($c.relaunch) { Start-Process -FilePath $c.updateExe -ArgumentList '--processStart Shellfox.exe' }
  Remove-Item -LiteralPath $c.work -Recurse -Force
} catch {
  try { $_ | Out-File -LiteralPath (Join-Path $c.work 'install.log') } catch {}
  if (-not $managerExited) { exit 1 }
  try { Add-Type -AssemblyName PresentationFramework; [System.Windows.MessageBox]::Show('Shellfox update failed. Reopen Shellfox and try again.', 'Shellfox update') | Out-Null } catch {}
  if ($c.relaunch) { try { Start-Process -FilePath $c.updateExe -ArgumentList '--processStart Shellfox.exe' } catch {} }
  exit 1
}
