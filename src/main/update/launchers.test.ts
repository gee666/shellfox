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
  });
  it.skipIf(process.platform !== 'linux')('routes updater help through the CLI and invalid update arguments to the updater, never launching the GUI', async () => {
    await mkdir(path.resolve('tmp'), { recursive: true });
    const root = await mkdtemp(path.resolve('tmp/update-launcher-'));
    try {
      const marker = path.join(root, 'launched'), exe = path.join(root, "app 'x'"), launcher = path.join(root, 'shellfox');
      const helper = path.join(root, 'helper'), args = path.join(root, 'helper-args');
      await writeFile(exe, `#!/bin/sh\nif [ "$1" = "$SHELLFOX_TEST_HELPER" ]; then printf '%s\\n' "$@" > "$SHELLFOX_TEST_ARGS"; printf '%s\\n' 'shellfox update [--check]'; exit 0; fi\necho launched > '${marker}'\n`); await chmod(exe, 0o755);
      const env = { ...process.env, SHELLFOX_TEST_HELPER: helper, SHELLFOX_TEST_ARGS: args };
      await writeFile(launcher, linuxLauncher({ executable: exe, cliHelper: helper })); await chmod(launcher, 0o755);
      const help = await new Promise<string>((resolve, reject) => execFile('/bin/sh', [launcher, 'update', '--help'], { timeout: 5000, env }, (error, out) => error ? reject(error) : resolve(out)));
      expect(help).toContain('shellfox update [--check]');
      expect(await readFile(args, 'utf8')).toBe([helper, 'update', '--help', ''].join('\n'));
      const bad = await new Promise<number>(resolve => execFile('/bin/sh', [launcher, 'update', '--nope'], { timeout: 5000, env }, error => resolve((error as { code?: number } | null)?.code ?? 0)));
      expect(bad).toBe(2);
      await expect(readFile(marker, 'utf8')).rejects.toMatchObject({ code: 'ENOENT' });
    } finally { await rm(root, { recursive: true, force: true }); }
  });
  it('routes `update` in the Windows cmd and POSIX shims to the PowerShell updater', () => {
    const shims = shellfoxShims({ executable: 'C:\\Users\\me\\AppData\\Local\\shellfox\\app-0.1.1\\Shellfox.exe', updateScript: updatePs1 });
    // cmd forwards argv to the bundled helper, which spawns the updater without cmd /c.
    const config = JSON.parse(Buffer.from(/SHELLFOX_CLI_LAUNCH=([^"\r\n]+)/.exec(shims.cmd)![1], 'base64').toString());
    expect(config).toEqual({ executable: 'C:\\Users\\me\\AppData\\Local\\shellfox\\app-0.1.1\\Shellfox.exe', args: [], env: {} });
    expect(shims.cmd).toContain('set "SHELLFOX_CLI_UPDATE=%~dp0shellfox-update.ps1"');
    expect(shims.cmd).toContain('cli\\main\\cli.cjs" %*');
    expect(shims.cmd).toContain('exit /b %errorlevel%');
    expect(shims.cmd).not.toContain('%~1'); expect(shims.cmd).not.toContain('%~2');
    expect(shims.posix).toContain('if [ "$1" = update ]; then');
    expect(shims.posix).toContain('-File "$script" -Executable \'C:\\Users\\me\\AppData\\Local\\shellfox\\app-0.1.1\\Shellfox.exe\'');
    expect(shims.posix).toContain('cygpath -w "$script"'); expect(shims.posix).toContain('wslpath -w "$script"');
    expect(shims.posix).toContain('shellfox-update.ps1');
    expect(shims.update).toBe(updatePs1);
  });
  it.skipIf(process.platform === 'win32')('generated POSIX launcher and shim pass /bin/sh syntax checks', () => {
    const launcher = linuxLauncher({ executable: '/Applications/Shellfox.app/Contents/MacOS/Shellfox' });
    const shims = shellfoxShims({ executable: 'C:\\Users\\me\\AppData\\Local\\shellfox\\app-0.1.1\\Shellfox.exe', updateScript: updatePs1 });
    for (const script of [launcher, shims.posix]) {
      const result = spawnSync('/bin/sh', ['-n'], { input: script, encoding: 'utf8', timeout: 5000 });
      expect(result.error).toBeUndefined();
      expect(result.status, result.stderr).toBe(0);
    }
  });
  it('the update scripts are bundled text with the documented overrides', () => {
    for (const text of [updateSh, updatePs1]) { expect(text).toContain('SHELLFOX_UPDATE_API'); expect(text).toContain('SHELLFOX_UPDATE_REPO'); expect(text).toContain('Restart Shellfox to use'); }
    expect(updatePs1).toContain('ShellfoxSetup.exe'); expect(updatePs1).toContain('Tls12'); expect(updatePs1).toContain('-UseBasicParsing');
  });
});
