import { describe, expect, it } from 'vitest';
import path from 'node:path';
import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { execFile, spawnSync } from 'node:child_process';
import { linuxLauncher } from '../platform/linux-cli';
import { shellfoxShims } from '../platform/shellfox-cli';
import updateSh from './shellfox-update.sh';
import updatePs1 from './shellfox-update.ps1';

describe('launchers route `update`', () => {
  it('embeds the sh updater in the generated POSIX launcher (Linux loose installs and macOS)', () => {
    const launcher = linuxLauncher({ executable: '/Applications/Shellfox.app/Contents/MacOS/Shellfox' });
    expect(launcher).toContain('shellfox update [--check]');
    expect(launcher).toContain('SHELLFOX_APP_EXECUTABLE=\'/Applications/Shellfox.app/Contents/MacOS/Shellfox\' exec /bin/sh -c "$shellfox_update_script" shellfox-update "$@"');
    expect(launcher).toContain('dpkg-query -W');
    expect(spawnSync('/bin/sh', ['-n'], { input: launcher }).status).toBe(0);
  });
  it.skipIf(process.platform !== 'linux')('the generated launcher runs the embedded updater with the app executable, never launching the app', async () => {
    await mkdir(path.resolve('tmp'), { recursive: true });
    const root = await mkdtemp(path.resolve('tmp/update-launcher-'));
    try {
      const marker = path.join(root, 'launched'), exe = path.join(root, "app 'x'"), launcher = path.join(root, 'shellfox');
      await writeFile(exe, `#!/bin/sh\necho launched > '${marker}'\n`); await chmod(exe, 0o755);
      await writeFile(launcher, linuxLauncher({ executable: exe })); await chmod(launcher, 0o755);
      const help = await new Promise<string>((resolve, reject) => execFile('/bin/sh', [launcher, 'update', '--help'], { timeout: 5000 }, (error, out) => error ? reject(error) : resolve(out)));
      expect(help).toContain('Usage: shellfox update [--check]');
      const bad = await new Promise<number>(resolve => execFile('/bin/sh', [launcher, 'update', '--nope'], { timeout: 5000 }, error => resolve((error as { code?: number } | null)?.code ?? 0)));
      expect(bad).toBe(2);
      await expect(readFile(marker, 'utf8')).rejects.toMatchObject({ code: 'ENOENT' });
    } finally { await rm(root, { recursive: true, force: true }); }
  });
  it('routes `update` in the Windows cmd and POSIX shims to the PowerShell updater', () => {
    const shims = shellfoxShims({ executable: 'C:\\Users\\me\\AppData\\Local\\shellfox\\app-0.1.1\\Shellfox.exe', updateScript: updatePs1 });
    const updateAt = shims.cmd.indexOf('if /i "%~1"=="update" goto update'), startAt = shims.cmd.indexOf('if /i not "%~1"=="start" goto invalid');
    expect(updateAt).toBeGreaterThan(0); expect(updateAt).toBeLessThan(startAt);
    expect(shims.cmd).toContain('powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0shellfox-update.ps1" -Executable "C:\\Users\\me\\AppData\\Local\\shellfox\\app-0.1.1\\Shellfox.exe" %shellfox_update_args%');
    expect(shims.cmd).toContain('exit /b %errorlevel%'); expect(shims.cmd).toContain('shellfox update [--check]');
    expect(shims.posix).toContain('if [ "$1" = update ]; then');
    expect(shims.posix).toContain('-File "$script" -Executable \'C:\\Users\\me\\AppData\\Local\\shellfox\\app-0.1.1\\Shellfox.exe\'');
    expect(shims.posix).toContain('cygpath -w "$script"'); expect(shims.posix).toContain('wslpath -w "$script"');
    expect(shims.posix).toContain('shellfox-update.ps1');
    expect(shims.update).toBe(updatePs1);
    expect(spawnSync('/bin/sh', ['-n'], { input: shims.posix }).status).toBe(0);
  });
  it('the update scripts are bundled text with the documented overrides', () => {
    for (const text of [updateSh, updatePs1]) { expect(text).toContain('SHELLFOX_UPDATE_API'); expect(text).toContain('SHELLFOX_UPDATE_REPO'); expect(text).toContain('Restart Shellfox to use'); }
    expect(updatePs1).toContain('ShellfoxSetup.exe'); expect(updatePs1).toContain('Tls12'); expect(updatePs1).toContain('-UseBasicParsing');
  });
});
