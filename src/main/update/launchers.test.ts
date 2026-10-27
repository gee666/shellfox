import { describe, expect, it } from 'vitest';
import path from 'node:path';
import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { execFile, spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { promisify } from 'node:util';
import { build } from 'esbuild';
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
  const shell = process.platform === 'win32' ? path.join(process.env.ProgramFiles ?? 'C:\\Program Files', 'Git', 'bin', 'bash.exe') : '/bin/sh';
  it.skipIf(!existsSync(shell))('routes help and JSON update requests through the universal CLI without launching GUI or the updater', async () => {
    await mkdir(path.resolve('tmp'), { recursive: true }); const root = await mkdtemp(path.resolve('tmp/update-launcher-'));
    const posix = (value: string) => process.platform === 'win32' ? value.replace(/^([A-Za-z]):/, (_, drive: string) => '/' + drive.toLowerCase()).replaceAll('\\', '/') : value;
    const exec = promisify(execFile);
    try {
      const marker = path.join(root, 'launched'), exe = path.join(root, "app 'x'"), launcher = path.join(root, 'shellfox'), helper = path.join(root, 'cli.cjs');
      await writeFile(exe, `#!/bin/sh
if [ "\${ELECTRON_RUN_AS_NODE:-}" = 1 ]; then
  helper=$1; shift
  if command -v cygpath >/dev/null 2>&1; then helper=$(cygpath -w "$helper") || exit 1; fi
  export MSYS_ARG_CONV_EXCL='*'
  exec "$SHELLFOX_TEST_NODE" "$helper" "$@"
fi
printf launched > "$SHELLFOX_TEST_GUI"
`); await chmod(exe, 0o755);
      await build({ entryPoints: ['src/main/platform/shellfox-cli-entry.ts'], outfile: helper, bundle: true, platform: 'node', format: 'cjs',
        packages: 'external', loader: { '.sh': 'text', '.ps1': 'text' }, define: { __SHELLFOX_VERSION__: '"test"' } });
      const env = { ...process.env, SHELLFOX_TEST_NODE: posix(process.execPath), SHELLFOX_TEST_GUI: posix(marker) };
      await writeFile(launcher, linuxLauncher({ executable: posix(exe), cliHelper: posix(helper), userData: path.join(root, 'private data') }));
      for (const args of [['update', '--help'], ['update', '-h'], ['update', 'help'], ['update', '--check', '--help']]) {
        const { stdout } = await exec(shell, [posix(launcher), ...args], { cwd: root, env, timeout: 10000 });
        expect(stdout).toContain('update [--check]'); expect(stdout).toContain('Update');
        expect(stdout).toContain('installs a verified version'); expect(stdout).toContain('never stop hosts');
      }
      for (const args of [['update', '--json'], ['--json', 'update'], ['update', '--check', '--json'], ['update', '--json', '--check'], ['--yes', 'update', '--json']]) {
        const result = await exec(shell, [posix(launcher), ...args], { cwd: root, env, timeout: 10000 }).catch((error: unknown) => {
          if (!(error instanceof Error) || !('code' in error) || !('stdout' in error) || !('stderr' in error)) throw error;
          return { code: error.code, stdout: String(error.stdout), stderr: String(error.stderr) };
        });
        expect('code' in result ? result.code : 0).toBe(2); expect(result.stderr).toBe('');
        expect(JSON.parse(result.stdout)).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
      }
      await expect(exec(shell, [posix(launcher), 'update', '--bad'], { cwd: root, env, timeout: 10000 })).rejects.toMatchObject({ code: 2 });
      await expect(readFile(marker, 'utf8')).rejects.toMatchObject({ code: 'ENOENT' });
    } finally { await rm(root, { recursive: true, force: true }); }
  }, 30000);
  it.skipIf(process.platform === 'win32')('runs POSIX desktop checks with global flags without contacting a host or installing', async () => {
    await mkdir('tmp', { recursive: true }); const root = await mkdtemp(path.resolve('tmp/posix-update-globals-'));
    const exec = promisify(execFile);
    try {
      const bin = path.join(root, 'bin'), log = path.join(root, 'requests'), api = path.join(root, 'release.json');
      const exe = path.join(root, 'Shellfox.app/Contents/MacOS/Shellfox'), helper = path.join(root, 'cli.cjs'), launcher = path.join(root, 'shellfox');
      await mkdir(bin); await mkdir(path.dirname(exe), { recursive: true });
      const fake = async (name: string, body: string) => {
        await writeFile(path.join(bin, name), '#!/bin/sh\n' + body + '\n'); await chmod(path.join(bin, name), 0o755);
      };
      await fake('dpkg-query', `if [ "$1" = -S ]; then printf 'shellfox: %s\\n' "$2"; else printf '0.9.5'; fi`);
      await fake('PlistBuddy', `case "$2" in *Identifier) echo local.shellfox;; *ShortVersion*) echo 0.9.5;; esac`);
      await fake('curl', `out=
while [ "$#" -gt 0 ]; do if [ "$1" = -o ]; then out=$2; shift; fi; shift; done
cp "$SHELLFOX_TEST_API" "$out" || exit 1
printf request >> "$SHELLFOX_TEST_REQUESTS"
printf 200`);
      await writeFile(path.join(root, 'Shellfox.app/Contents/Info.plist'), 'local fixture');
      await writeFile(api, JSON.stringify({ tag_name: 'v0.9.6', assets: [] }));
      await writeFile(exe, '#!/bin/sh\nhelper=$1; shift\nexec "$SHELLFOX_TEST_NODE" "$helper" "$@"\n'); await chmod(exe, 0o755);
      await build({ entryPoints: ['src/main/platform/shellfox-cli-entry.ts'], outfile: helper, bundle: true, platform: 'node', format: 'cjs',
        packages: 'external', loader: { '.sh': 'text' }, define: { __SHELLFOX_VERSION__: '"0.9.5"' } });
      await writeFile(launcher, linuxLauncher({ executable: exe, cliHelper: helper }));
      const env = { ...process.env, PATH: bin + ':' + process.env.PATH, TMPDIR: root, SHELLFOX_TEST_NODE: process.execPath,
        SHELLFOX_TEST_API: api, SHELLFOX_TEST_REQUESTS: log, SHELLFOX_PLISTBUDDY: path.join(bin, 'PlistBuddy') };
      for (const args of [
        ['update', '--check'], ['--host', 'local', 'update', '--check'], ['update', '--check', '--host=local'],
        ['--yes', 'update', '--check'], ['--user-data', path.join(root, 'private data'), 'update', '--check'],
      ]) {
        const result = await exec('/bin/sh', [launcher, ...args], { cwd: root, env, timeout: 10_000 });
        expect(result.stdout).toContain('Shellfox 0.9.6 is available'); expect(result.stderr).toBe('');
      }
      expect(await readFile(log, 'utf8')).toBe('request'.repeat(5));
    } finally { await rm(root, { recursive: true, force: true }); }
  }, 30_000);
  it('routes `update` in the Windows cmd and POSIX shims to the PowerShell updater', () => {
    const shims = shellfoxShims({ executable: 'C:\\Users\\me\\AppData\\Local\\shellfox\\app-0.1.1\\Shellfox.exe', updateScript: updatePs1 });
    // cmd forwards argv to the bundled helper, which spawns the updater without cmd /c.
    const config = JSON.parse(Buffer.from(/SHELLFOX_CLI_LAUNCH=([^"\r\n]+)/.exec(shims.cmd)![1], 'base64').toString());
    expect(config).toEqual({ executable: 'C:\\Users\\me\\AppData\\Local\\shellfox\\app-0.1.1\\Shellfox.exe', args: [], env: {} });
    expect(shims.cmd).toContain('set "SHELLFOX_CLI_UPDATE=%~dp0shellfox-update.ps1"');
    expect(shims.cmd).toContain('cli\\main\\cli.cjs" %*');
    expect(shims.cmd).toContain('exit /b %errorlevel%');
    expect(shims.cmd).not.toContain('%~1'); expect(shims.cmd).not.toContain('%~2');
    expect(shims.posix).toContain('export SHELLFOX_CLI_UPDATE="$shellfox_update"');
    expect(shims.posix).toContain('exec "$runtime" "$helper"');
    expect(shims.posix).toContain('cygpath -w "$shellfox_update"'); expect(shims.posix).toContain('wslpath -w "$shellfox_update"');
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
