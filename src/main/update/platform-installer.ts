import { execFile } from 'node:child_process';
import { constants } from 'node:fs';
import { access, mkdtemp, readFile, realpath, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { promisify } from 'node:util';
import { hashFile, type UpdatePlatform } from './self-updater';
import { validateUpdateZip } from './zip-validation';
import { posixCommitGate, powershellCommitGate, startHandoff } from './handoff';
const exec = promisify(execFile);
const run = async (file: string, args: string[]) => (await exec(file, args, { timeout: 60_000, windowsHide: true, maxBuffer: 1024 * 1024 })).stdout.trim();
const waitForExit = `tries=0
while kill -0 "$pid" 2>/dev/null; do
  [ ! -f "$control/cancel" ] || exit 1
  tries=$((tries + 1))
  [ "$tries" -le 120 ] || exit 1
  sleep 1
done
[ ! -f "$control/cancel" ] || exit 1
`;
const linuxHelper = `#!/bin/sh
pid=$1; work=$2; package=$3; executable=$4; expected=$5; control=$6
exec >"$work/install.log" 2>&1
failed() {
  if [ -x /usr/bin/zenity ]; then /usr/bin/zenity --error --text='Shellfox update failed or was cancelled. Reopen Shellfox to try again.'; fi
  "$executable" >/dev/null 2>&1 &
  exit 1
}
actual=$(/usr/bin/sha256sum -- "$package") || exit 1
[ "\${actual%% *}" = "$expected" ] || exit 1
${posixCommitGate}
${waitForExit}
actual=$(/usr/bin/sha256sum -- "$package") || failed
[ "\${actual%% *}" = "$expected" ] || failed
if /usr/bin/pkexec /usr/bin/apt-get install -y "$package"; then
  "$executable" >/dev/null 2>&1 &
  rm -rf -- "$work"
else
  failed
fi
`;
const macHelper = `#!/bin/sh
pid=$1; work=$2; bundle=$3; stage=$4; control=$5
exec >"$work/install.log" 2>&1
failed() {
  /usr/bin/osascript -e 'display alert "Shellfox update failed" message "The previous app was kept. Reopen Shellfox and try again."' || true
  /usr/bin/open -n "$bundle"
  exit 1
}
/usr/bin/codesign --verify --deep --strict "$stage/Shellfox.app" || exit 1
${posixCommitGate}
${waitForExit}
old="$stage/previous.app"
new="$stage/Shellfox.app"
/usr/bin/codesign --verify --deep --strict "$new" || failed
if mv -- "$bundle" "$old"; then
  if mv -- "$new" "$bundle"; then
    /usr/bin/open -n "$bundle"
    rm -rf -- "$stage" "$work"
    exit 0
  fi
  if ! mv -- "$old" "$bundle"; then
    /usr/bin/osascript -e 'display alert "Shellfox rollback failed" message "The previous app is in the .Shellfox-update backup folder beside the installation. Restore it manually."' || true
    exit 1
  fi
fi
failed
`;
const windowsHelper = `param([string]$Config)
$ErrorActionPreference = 'Stop'
$c = $null
$managerExited = $false
try {
  $c = Get-Content -LiteralPath $Config -Raw -Encoding UTF8 | ConvertFrom-Json
  if ((Get-FileHash -LiteralPath $c.file -Algorithm SHA256).Hash -ne $c.sha256) { throw 'Installer checksum changed.' }
  ${powershellCommitGate}
  $p = Get-Process -Id $c.pid -ErrorAction SilentlyContinue
  if ($p) { Wait-Process -Id $c.pid -Timeout 120 -ErrorAction Stop }
  if (Test-Path -LiteralPath (Join-Path $c.control 'cancel')) { exit 1 }
  $managerExited = $true
  $hash = (Get-FileHash -LiteralPath $c.file -Algorithm SHA256).Hash
  if ($hash -ne $c.sha256) { throw 'Installer checksum changed.' }
  $install = Start-Process -FilePath $c.file -ArgumentList '--silent' -Wait -PassThru
  if ($install.ExitCode -ne 0) { throw "Installer exited with $($install.ExitCode)" }
  Start-Process -FilePath $c.updateExe -ArgumentList '--processStart Shellfox.exe'
  Remove-Item -LiteralPath $c.work -Recurse -Force
} catch {
  try { $_ | Out-File -LiteralPath (Join-Path $c.work 'install.log') } catch {}
  if (-not $managerExited) { exit 1 }
  try { Add-Type -AssemblyName PresentationFramework; [System.Windows.MessageBox]::Show('Shellfox update failed. Reopen Shellfox and try again.', 'Shellfox update') | Out-Null } catch {}
  try { Start-Process -FilePath $c.updateExe -ArgumentList '--processStart Shellfox.exe' } catch {}
  exit 1
}
`;

export async function createUpdatePlatform(options: { platform: string; arch: string; executable: string; packaged: boolean; pid?: number }): Promise<UpdatePlatform> {
  const { platform, arch, executable } = options;
  const pid = options.pid ?? process.pid;
  const assetName = (v: string) => platform === 'win32' ? 'ShellfoxSetup.exe' : platform === 'linux' ? `shellfox_${v}_${arch === 'x64' ? 'amd64' : 'arm64'}.deb` : `Shellfox-darwin-${arch}-${v}.zip`;
  const unsupported = (reason: string): UpdatePlatform => ({ supported: false, reason, assetName, prepare: async () => { throw new Error(reason); } });
  if (!options.packaged) return unsupported('Self-update is available only in installed builds.');
  if (!['x64', 'arm64'].includes(arch)) return unsupported('No installer is published for this architecture.');
  if (platform === 'win32') {
    const appDir = path.win32.dirname(executable), root = path.win32.dirname(appDir), updateExe = path.win32.join(root, 'Update.exe');
    if (arch !== 'x64' || !/^app-\d[\w.+-]*$/.test(path.win32.basename(appDir))) return unsupported('Windows ZIP and ARM64 builds must be updated manually.');
    try { await access(updateExe); } catch { return unsupported('This copy was not installed with ShellfoxSetup.exe. Update it manually.'); }
    const powershell = path.win32.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/WindowsPowerShell/v1.0/powershell.exe');
    return { supported: true, reason: null, assetName, prepare: async (file, _version, work) => {
      const helper = path.join(work, 'install.ps1'), sha256 = await hashFile(file);
      await writeFile(helper, windowsHelper, { flag: 'wx', mode: 0o600 });
      return async () => {
        const control = await mkdtemp(path.join(work, 'handoff-')), config = path.join(control, 'install.json');
        await writeFile(config, JSON.stringify({ pid, file, work, updateExe, sha256, control }), { flag: 'wx', mode: 0o600 });
        return startHandoff(powershell, ['-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', helper, '-Config', config], control);
      };
    } };
  }
  if (platform === 'linux') {
    try {
      const resolved = await realpath(executable);
      const owned = (await run('/usr/bin/dpkg-query', ['-S', resolved])).split('\n').some(line => line === `shellfox: ${resolved}`);
      if (!owned) return unsupported('Only .deb installations support in-app updates on Linux.');
      await access('/usr/bin/pkexec', constants.X_OK); await access('/usr/bin/apt-get', constants.X_OK); await access('/usr/bin/sha256sum', constants.X_OK);
    } catch { return unsupported('In-app updates require a .deb installation, apt-get and graphical PolicyKit authentication.'); }
    return { supported: true, reason: null, assetName, prepare: async (file, version, work) => {
      const name = await run('/usr/bin/dpkg-deb', ['-f', file, 'Package']);
      const actualVersion = await run('/usr/bin/dpkg-deb', ['-f', file, 'Version']);
      const actualArch = await run('/usr/bin/dpkg-deb', ['-f', file, 'Architecture']);
      if (name !== 'shellfox' || actualVersion !== version || actualArch !== (arch === 'x64' ? 'amd64' : 'arm64')) throw new Error('Unexpected Debian package.');
      const helper = path.join(work, 'install.sh');
      await writeFile(helper, linuxHelper, { flag: 'wx', mode: 0o700 });
      const digest = await hashFile(file);
      return async () => {
        const control = await mkdtemp(path.join(work, 'handoff-'));
        return startHandoff('/bin/sh', [helper, String(pid), work, file, executable, digest, control], control);
      };
    } };
  }
  if (platform === 'darwin') {
    const suffix = '/Contents/MacOS/Shellfox';
    if (!executable.endsWith(suffix)) return unsupported('Only Shellfox.app bundles support in-app updates on macOS.');
    const bundle = executable.slice(0, -suffix.length), parent = path.dirname(bundle);
    try {
      if (await realpath(bundle) !== bundle || await run('/usr/libexec/PlistBuddy', ['-c', 'Print :CFBundleIdentifier', path.join(bundle, 'Contents/Info.plist')]) !== 'local.shellfox') throw new Error();
      await access(parent, constants.W_OK);
    } catch { return unsupported('Move Shellfox.app to a writable Applications folder before updating.'); }
    let stagedPath: string | null = null;
    return { supported: true, reason: null, assetName,
      cleanup: async () => { if (stagedPath) await rm(stagedPath, { recursive: true, force: true }); stagedPath = null; },
      prepare: async (file, version, work) => {
      validateUpdateZip(await readFile(file));
      const extracted = path.join(work, 'extracted');
      await run('/usr/bin/ditto', ['-x', '-k', file, extracted]);
      const newApp = path.join(extracted, 'Shellfox.app'), plist = path.join(newApp, 'Contents/Info.plist');
      if (await run('/usr/libexec/PlistBuddy', ['-c', 'Print :CFBundleIdentifier', plist]) !== 'local.shellfox' || await run('/usr/libexec/PlistBuddy', ['-c', 'Print :CFBundleShortVersionString', plist]) !== version) throw new Error('Unexpected app bundle.');
      await run('/usr/bin/codesign', ['--verify', '--deep', '--strict', newApp]);
      // Stage on the same filesystem so replacement and rollback use atomic renames.
      const stage = await mkdtemp(path.join(parent, '.Shellfox-update-'));
      stagedPath = stage;
      try {
        await run('/usr/bin/ditto', [newApp, path.join(stage, 'Shellfox.app')]);
        await run('/usr/bin/codesign', ['--verify', '--deep', '--strict', path.join(stage, 'Shellfox.app')]);
        const helper = path.join(work, 'install.sh');
        await writeFile(helper, macHelper, { flag: 'wx', mode: 0o700 });
        return async () => {
          const control = await mkdtemp(path.join(work, 'handoff-'));
          return startHandoff('/bin/sh', [helper, String(pid), work, bundle, stage, control], control);
        };
      } catch (error) { await rm(stage, { recursive: true, force: true }); throw error; }
    } };
  }
  return unsupported('In-app updates are not supported on this platform.');
}
