import { execFile } from 'node:child_process';
import { constants } from 'node:fs';
import { access, mkdtemp, readFile, realpath, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { promisify } from 'node:util';
import { hashFile, type UpdatePlatform } from './self-updater';
import { validateUpdateZip } from './zip-validation';
import { startHandoff } from './handoff';
import linuxHelper from './install-linux.sh';
import macHelper from './install-mac.sh';
import windowsHelper from './install-windows.ps1';
const exec = promisify(execFile);
const run = async (file: string, args: string[]) => (await exec(file, args, { timeout: 60_000, windowsHide: true, maxBuffer: 1024 * 1024 })).stdout.trim();
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
      return async (relaunch = true) => {
        const control = await mkdtemp(path.join(work, 'handoff-')), config = path.join(control, 'install.json');
        await writeFile(config, JSON.stringify({ pid, file, work, updateExe, sha256, control, relaunch }), { flag: 'wx', mode: 0o600 });
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
      return async (relaunch = true) => {
        const control = await mkdtemp(path.join(work, 'handoff-'));
        return startHandoff('/bin/sh', [helper, String(pid), work, file, executable, digest, control, relaunch ? '1' : '0'], control);
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
        return async (relaunch = true) => {
          const control = await mkdtemp(path.join(work, 'handoff-'));
          return startHandoff('/bin/sh', [helper, String(pid), work, bundle, stage, control, relaunch ? '1' : '0'], control);
        };
      } catch (error) { await rm(stage, { recursive: true, force: true }); throw error; }
    } };
  }
  return unsupported('In-app updates are not supported on this platform.');
}
