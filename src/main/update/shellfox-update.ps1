# Shellfox updater for Windows (Squirrel installs). Compatible with Windows PowerShell 5.1.
# Usage: shellfox update [--check]
# Test overrides: SHELLFOX_UPDATE_API (latest-release JSON URL), SHELLFOX_UPDATE_REPO (owner/name).
param(
  [string]$Executable = '',
  [switch]$Check,
  [switch]$Help,
  [Parameter(ValueFromRemainingArguments = $true)][string[]]$Rest
)
$ErrorActionPreference = 'Stop'
$Repo = 'gee666/shellfox'
if ($env:SHELLFOX_UPDATE_REPO) { $Repo = $env:SHELLFOX_UPDATE_REPO }
$Api = "https://api.github.com/repos/$Repo/releases/latest"
if ($env:SHELLFOX_UPDATE_API) { $Api = $env:SHELLFOX_UPDATE_API }
$ReleasesUrl = "https://github.com/$Repo/releases"

function Show-Usage {
  Write-Output 'Usage: shellfox update [--check]'
  Write-Output 'Downloads and installs the latest published Shellfox release.'
  Write-Output '  --check   only report whether a newer version is available'
}
function Stop-Update([string]$Message, [int]$Code = 1) {
  [Console]::Error.WriteLine('Shellfox update: ' + $Message)
  exit $Code
}
function Get-VersionCore([string]$Version) {
  $core = $Version.Trim()
  if ($core.StartsWith('v')) { $core = $core.Substring(1) }
  $cut = $core.IndexOfAny([char[]]@('-', '+'))
  if ($cut -ge 0) { $core = $core.Substring(0, $cut) }
  return $core
}
function Get-VersionPart([string[]]$Parts, [int]$Index) {
  if ($Index -ge $Parts.Length) { return [decimal]0 }
  if ($Parts[$Index] -match '^\d+') { return [decimal]$Matches[0] }
  return [decimal]0
}
# Same semantics as shellfox-update.sh: dotted numeric compare, -/+ suffixes ignored.
function Test-Newer([string]$Candidate, [string]$Current) {
  $a = (Get-VersionCore $Candidate).Split('.')
  $b = (Get-VersionCore $Current).Split('.')
  $count = [Math]::Max($a.Length, $b.Length)
  for ($i = 0; $i -lt $count; $i++) {
    $x = Get-VersionPart $a $i
    $y = Get-VersionPart $b $i
    if ($x -gt $y) { return $true }
    if ($x -lt $y) { return $false }
  }
  return $false
}
function Test-IsFileUri([string]$Url) {
  $uri = $null
  return ([Uri]::TryCreate($Url, [UriKind]::Absolute, [ref]$uri) -and $uri.IsFile)
}

foreach ($argument in @($Rest)) {
  if ($argument -eq '--check') { $Check = [switch]$true }
  elseif ($argument -eq '--help' -or $argument -eq '-h') { $Help = [switch]$true }
  elseif ($argument) { Show-Usage; exit 2 }
}
if ($Help) { Show-Usage; exit 0 }

try { [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 } catch { }
$ProgressPreference = 'SilentlyContinue'

# Only Squirrel installs (<root>\app-<version>\Shellfox.exe next to <root>\Update.exe) can update themselves.
$notSquirrel = "this Shellfox was not installed with ShellfoxSetup.exe (zip and arm64 installs cannot update themselves). Download the latest release from $ReleasesUrl"
if (-not $Executable -or -not (Test-Path -LiteralPath $Executable -PathType Leaf)) { Stop-Update $notSquirrel }
$appDir = Split-Path -Parent $Executable
$installRoot = Split-Path -Parent $appDir
if (-not $installRoot -or -not ((Split-Path -Leaf $appDir) -like 'app-*') -or -not (Test-Path -LiteralPath (Join-Path $installRoot 'Update.exe') -PathType Leaf)) { Stop-Update $notSquirrel }

$current = ''
try { $current = [string](Get-Item -LiteralPath $Executable).VersionInfo.ProductVersion } catch { }
if (-not $current) { $current = (Split-Path -Leaf $appDir).Substring(4) }
$current = $current.Trim()
Write-Output "Installed Shellfox version: $current"

$headers = @{ 'User-Agent' = 'shellfox-update'; 'Accept' = 'application/vnd.github+json' }
$json = $null
try {
  if (Test-IsFileUri $Api) { $json = Get-Content -LiteralPath ([Uri]$Api).LocalPath -Raw }
  else { $json = (Invoke-WebRequest -UseBasicParsing -Headers $headers -Uri $Api -TimeoutSec 60).Content }
} catch {
  $status = 0
  try { $status = [int]$_.Exception.Response.StatusCode } catch { }
  if ($status -eq 404) { Write-Output 'No published Shellfox release yet.'; exit 0 }
  Stop-Update "could not query the latest release ($Api). Check your network connection, or see $ReleasesUrl"
}
$release = $null
try { $release = $json | ConvertFrom-Json } catch { }
if (-not $release -or -not $release.tag_name) { Stop-Update "the release information did not contain a version. See $ReleasesUrl" }
$tag = [string]$release.tag_name
$latest = $tag
if ($latest.StartsWith('v')) { $latest = $latest.Substring(1) }

if (-not (Test-Newer $latest $current)) { Write-Output "Shellfox is up to date ($current)."; exit 0 }
if ($Check) { Write-Output "Shellfox $latest is available (installed: $current). Run: shellfox update"; exit 0 }
Write-Output "Updating Shellfox $current -> $latest"

$asset = @($release.assets) | Where-Object { $_.name -eq 'ShellfoxSetup.exe' } | Select-Object -First 1
if (-not $asset -or -not $asset.browser_download_url) { Stop-Update "release $tag has no ShellfoxSetup.exe. See $ReleasesUrl" }

$work = Join-Path ([IO.Path]::GetTempPath()) ('shellfox-update-' + [Guid]::NewGuid().ToString('N'))
$exitCode = 1
try {
  New-Item -ItemType Directory -Path $work | Out-Null
  $setup = Join-Path $work 'ShellfoxSetup.exe'
  Write-Output 'Downloading ShellfoxSetup.exe'
  try {
    if (Test-IsFileUri ([string]$asset.browser_download_url)) { Copy-Item -LiteralPath ([Uri][string]$asset.browser_download_url).LocalPath -Destination $setup }
    else { Invoke-WebRequest -UseBasicParsing -Headers @{ 'User-Agent' = 'shellfox-update' } -Uri ([string]$asset.browser_download_url) -OutFile $setup }
  } catch { Stop-Update "download failed: $($asset.browser_download_url)" }
  if (-not (Test-Path -LiteralPath $setup) -or (Get-Item -LiteralPath $setup).Length -eq 0) { Stop-Update 'the downloaded installer is empty.' }
  Write-Output 'Running the installer...'
  # -Wait waits for the entire process tree on Windows, including Shellfox
  # launched by Squirrel after installation. Wait for only the installer.
  $process = Start-Process -FilePath $setup -PassThru
  try {
    # Cache the handle before it exits so PowerShell 5.1 retains the exit code.
    [void]$process.Handle
    $process.WaitForExit()
    $installerExitCode = $process.ExitCode
  } finally { $process.Dispose() }
  if ($installerExitCode -ne 0) { Stop-Update "the installer failed (exit code $installerExitCode)." }
  Write-Output "Shellfox $latest installed."
  Write-Output "Restart Shellfox to use $latest (quitting Shellfox closes its terminals)."
  $exitCode = 0
} finally {
  Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}
exit $exitCode
