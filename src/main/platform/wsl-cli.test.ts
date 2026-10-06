import { afterEach, describe, expect, it, vi } from 'vitest';
import { execFile, execFileSync } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdir, mkdtemp, readFile, rm, writeFile, access, symlink, link, chmod, stat, readdir } from 'node:fs/promises';
import path from 'node:path';
import { setWslCli, WSL_STARTUP_SCRIPT, type WslCliRunner } from './wsl-cli';
import { cliIntegrationSchema } from '../../shared/schemas';

const ok = Buffer.from(JSON.stringify({ errors: [], updated: 2 }));
function runner(list = 'Debian\nUbuntu\n'): WslCliRunner {
  return vi.fn(async (_file, args) => {
    if (args.includes('--list')) return Buffer.from('\uFEFF' + list, 'utf16le');
    if (args.includes('wslpath')) return Buffer.from("/mnt/c/Users/O'Brien $(touch nope)/Shellfox bin\n");
    return ok;
  });
}
it('enumerates UTF-16 distros once, deduplicates and passes all values as script arguments', async () => {
  const run = runner('Debian\nUbuntu\nDebian\n');
  const bin = "C:\\Users\\O'Brien $(touch nope)\\Shellfox bin";
  expect(await setWslCli(true, bin, { run, home: '/tmp/isolated shellfox' })).toContain('Restart WSL');
  const calls = vi.mocked(run).mock.calls;
  expect(calls).toHaveLength(5);
  expect(calls[0][1]).toEqual(['--list', '--quiet']);
  for (const distro of ['Debian', 'Ubuntu']) {
    expect(calls).toContainEqual([expect.stringMatching(/System32\\wsl.exe$/), ['--distribution', distro, '--exec', 'wslpath', '-a', '-u', bin]]);
    expect(calls).toContainEqual([expect.any(String), ['--distribution', distro, '--exec', '/usr/bin/env', 'python3', '-c', WSL_STARTUP_SCRIPT, 'enable', "/mnt/c/Users/O'Brien $(touch nope)/Shellfox bin", '/tmp/isolated shellfox']]);
  }
  expect(WSL_STARTUP_SCRIPT).not.toContain(bin);
});
it('removes blocks without requiring wslpath or the Windows bin to exist', async () => {
  const run = runner('Debian\n');
  await setWslCli(false, 'C:\\missing bin', { run });
  expect(vi.mocked(run).mock.calls).toHaveLength(2);
  expect(vi.mocked(run).mock.calls[1][1].slice(-3)).toEqual(['disable', '', '']);
});
it('isolates a failed distro and reports partial failure without throwing', async () => {
  const base = runner();
  const run = vi.fn(async (file: string, args: string[]) => {
    if (args.includes('Debian')) throw new Error('Guest unavailable');
    return base(file, args);
  });
  expect(await setWslCli(true, 'C:\\bin', { run })).toContain('incomplete for Debian');
  expect(run.mock.calls.some(([, args]) => args.includes('Ubuntu') && args.includes(WSL_STARTUP_SCRIPT))).toBe(true);
});
it('bounds warnings to the CLI schema limit even when all 32 long distro names fail', async () => {
  const names = Array.from({ length: 32 }, (_, index) => String(index).padStart(3, '0') + 'x'.repeat(157));
  const run = vi.fn(async (_file: string, args: string[]) => {
    if (args.includes('--list')) return Buffer.from(names.join('\n'));
    throw new Error('Guest unavailable');
  });
  const reason = await setWslCli(true, 'C:\\bin', { run });
  expect(reason).toHaveLength(1000);
  expect(reason).toContain('...');
  expect(reason).toContain('Windows CLI is unaffected; restart WSL shells after retrying.');
  expect(cliIntegrationSchema.safeParse({ supported: true, installed: true, command: 'shellfox start <path>', reason }).success).toBe(true);
  expect(run).toHaveBeenCalledTimes(33);
});
it('reports unavailable WSL, unsafe input and invalid guest output without throwing', async () => {
  const unavailable = vi.fn(async () => { throw new Error('No WSL'); });
  expect(await setWslCli(true, 'C:\\bin', { run: unavailable })).toContain('Windows CLI is unaffected');
  const run = runner();
  expect(await setWslCli(true, 'C:\\bin', { run, home: 'relative' })).toContain('invalid isolated guest home');
  expect(run).not.toHaveBeenCalled();
  for (const output of ['not json', '{}', '{"errors":[".bashrc"]}']) {
    const run = vi.fn(async (_file: string, args: string[]) => args.includes('--list') ? Buffer.from('Debian\n') : args.includes('wslpath') ? Buffer.from('/mnt/c/bin\n') : Buffer.from(output));
    expect(await setWslCli(true, 'C:\\bin', { run })).toContain('incomplete for Debian');
  }
});
it('does nothing for an empty list and rejects unrepresentable PATH entries', async () => {
  const run = runner('');
  expect(await setWslCli(true, 'C:\\bin', { run })).toBeNull();
  expect(run).toHaveBeenCalledTimes(1);
  const unsafe = vi.fn(async (_file: string, args: string[]) => args.includes('--list') ? Buffer.from('Debian\n') : Buffer.from('/mnt/c/bin:bad\n'));
  expect(await setWslCli(true, 'C:\\bin', { run: unsafe })).toContain('incomplete');
  expect(unsafe).toHaveBeenCalledTimes(2);
});

// Exercise the exact guest editor in isolated native directories, not a real WSL HOME.
// Windows uses the Python launcher if installed. These tests never invoke wsl.exe.
const exec = promisify(execFile);
const python = process.platform === 'win32' ? 'py' : 'python3';
const pythonArgs = process.platform === 'win32' ? ['-3'] : [];
// Never resolve ambient bash.exe on Windows: it might be the WSL forwarding stub.
const bash = process.platform === 'win32' ? path.win32.join(process.env.ProgramFiles || 'C:\\Program Files', 'Git', 'usr', 'bin', 'bash.exe') : '/bin/bash';
let hasBash = false;
try { execFileSync(bash, ['--version'], { timeout: 3000, windowsHide: true, stdio: 'ignore' }); hasBash = true; } catch { /* Optional native Bash. */ }
let hasPython = false;
try { execFileSync(python, [...pythonArgs, '--version'], { timeout: 3000, windowsHide: true, stdio: 'ignore' }); hasPython = true; } catch { /* Optional local interpreter. */ }
const directories: string[] = [];
afterEach(async () => { await Promise.all(directories.splice(0).map(dir => rm(dir, { recursive: true, force: true }))); });
async function fixture() {
  await mkdir(path.resolve('tmp'), { recursive: true });
  const home = await mkdtemp(path.resolve('tmp/wsl-cli-'));
  directories.push(home);
  return home;
}
async function edit(home: string, enabled: boolean, bin = "/mnt/c/O'Brien $(touch nope)/Shellfox bin", zsh = true, injection = '') {
  // Shell availability is deterministic regardless of the host's installed shells.
  const code = `import shutil\nshutil.which = lambda name: '/bin/' + name if name == 'bash' or ${zsh ? 'True' : 'False'} else None\n` + injection + '\n' + WSL_STARTUP_SCRIPT;
  return JSON.parse((await exec(python, [...pythonArgs, '-c', code, enabled ? 'enable' : 'disable', bin, home], { timeout: 8000, windowsHide: true })).stdout) as { errors: string[]; updated: number };
}
describe.skipIf(!hasPython)('guest startup editor', () => {
  it('preserves foreign bytes and mode, enables login/nonlogin bash and zsh, is idempotent and reversible', async () => {
    const home = await fixture();
    const original = Buffer.from('# user config\r\nexport PATH=/usr/bin:$PATH');
    await writeFile(path.join(home, '.profile'), original);
    await chmod(path.join(home, '.profile'), 0o640);
    const mode = (await stat(path.join(home, '.profile'))).mode;
    expect(await edit(home, true)).toEqual({ errors: [], updated: 5 });
    expect((await stat(path.join(home, '.profile'))).mode).toBe(mode);
    expect(await access(path.join(home, '.bash_profile')).then(() => true, () => false)).toBe(false);
    const content = await readFile(path.join(home, '.profile'), 'utf8');
    expect(content.startsWith(original.toString())).toBe(true);
    expect(content).toContain("'/mnt/c/O'\\''Brien $(touch nope)/Shellfox bin'");
    expect(content).toContain('hash -r');
    expect(await edit(home, true)).toEqual({ errors: [], updated: 0 });
    expect(await edit(home, false)).toEqual({ errors: [], updated: 5 });
    expect(await readFile(path.join(home, '.profile'))).toEqual(original);
    for (const name of ['.bashrc', '.zprofile', '.zshrc', '.zlogin']) {
      expect(await access(path.join(home, name)).then(() => true, () => false)).toBe(false);
    }
  });
  it.skipIf(!hasBash)('prepends the Windows bin ahead of an old Linux launcher and clears command hashing when sourced', async () => {
    const home = await fixture();
    const bin = path.join(home, "Windows O'Brien $(touch nope) bin");
    const old = path.join(home, 'old-linux-bin');
    await mkdir(bin); await mkdir(old);
    for (const directory of [bin, old]) {
      await writeFile(path.join(directory, 'shellfox'), '#!/bin/sh\nexit 0\n', { mode: 0o755 });
    }
    const posix = (value: string) => process.platform === 'win32' ? value.replace(/^([A-Za-z]):/, (_, drive: string) => '/' + drive.toLowerCase()).replaceAll('\\', '/') : value;
    await edit(home, true, posix(bin));
    // Native Bash or Git Bash, not WSL. All paths remain separate argv values.
    const script = 'PATH="$1:$PATH"; shellfox; . "$2"; . "$2"; command -v shellfox; printf "%s\\n" "$PATH"';
    const { stdout } = await exec(bash, ['--noprofile', '--norc', '-c', script, 'shellfox-test', posix(old), posix(path.join(home, '.bashrc'))], { timeout: 8000, windowsHide: true });
    const lines = stdout.trimEnd().split(/\r?\n/);
    expect(lines[0]).toBe(posix(bin) + '/shellfox');
    expect(lines[1].split(':').filter(entry => entry === posix(bin))).toHaveLength(1);
  });
  it('selects bash login precedence without hiding .profile, and cleans stale inactive blocks', async () => {
    const home = await fixture();
    await writeFile(path.join(home, '.profile'), 'keep profile\n');
    await writeFile(path.join(home, '.bash_login'), 'keep login\n');
    await edit(home, true, '/mnt/c/bin', false);
    expect(await readFile(path.join(home, '.profile'), 'utf8')).toBe('keep profile\n');
    expect(await readFile(path.join(home, '.bash_login'), 'utf8')).toContain('Shellfox/wsl-cli-v1');
    await writeFile(path.join(home, '.bash_profile'), 'new preferred login\n');
    await edit(home, true, '/mnt/c/new bin', false);
    expect(await readFile(path.join(home, '.bash_login'), 'utf8')).toBe('keep login\n');
    expect(await readFile(path.join(home, '.bash_profile'), 'utf8')).toContain('/mnt/c/new bin');
    await edit(home, false);
    expect(await readFile(path.join(home, '.bash_profile'), 'utf8')).toBe('new preferred login\n');
  });
  for (const failure of ['partial write', 'fsync', 'replace'] as const) {
    it(`leaves originals unchanged and cleans stages after injected ${failure} failure`, async () => {
      const home = await fixture();
      const original = Buffer.from('# foreign config\r\nexport PATH=/usr/bin:$PATH');
      for (const name of ['.bashrc', '.profile']) {
        await writeFile(path.join(home, name), original);
        await chmod(path.join(home, name), 0o640);
      }
      const injection = 'import os, errno\n' + (failure === 'partial write' ? `
real_write = os.write
def fail_write(fd, data):
    real_write(fd, data[:7])
    raise OSError(errno.ENOSPC, 'Injected partial write')
os.write = fail_write
` : failure === 'fsync' ? `
def fail_fsync(fd):
    raise OSError(errno.ENOSPC, 'Injected fsync failure')
os.fsync = fail_fsync
` : `
def fail_replace(src, dst):
    raise OSError(errno.EACCES, 'Injected rename failure')
os.replace = fail_replace
`);
      // Cover initial enable, replacing an existing managed block, and removal.
      for (const operation of ['enable', 'update', 'disable'] as const) {
        const before = await Promise.all(['.bashrc', '.profile'].map(async name => ({
          name, bytes: await readFile(path.join(home, name)), metadata: await stat(path.join(home, name)),
        })));
        expect(await edit(home, operation !== 'disable', '/mnt/c/new-bin', false, injection)).toEqual({ errors: ['.bashrc', '.profile'], updated: 0 });
        for (const { name, bytes, metadata } of before) {
          expect(await readFile(path.join(home, name))).toEqual(bytes);
          const after = await stat(path.join(home, name));
          expect(after.ino).toBe(metadata.ino);
          expect(after.mode).toBe(metadata.mode);
        }
        expect((await readdir(home)).sort()).toEqual(['.bashrc', '.profile']);
        if (operation === 'enable') expect(await edit(home, true, '/mnt/c/bin', false)).toEqual({ errors: [], updated: 2 });
      }
      expect(await edit(home, false, '', false)).toEqual({ errors: [], updated: 2 });
      expect(await readFile(path.join(home, '.bashrc'))).toEqual(original);
      expect(await readFile(path.join(home, '.profile'))).toEqual(original);
    });
  }
  it('cleans incomplete stages without creating startup files when a new-file write fails', async () => {
    const home = await fixture();
    const injection = `
import os, errno
real_write = os.write
def fail_write(fd, data):
    real_write(fd, data[:7])
    raise OSError(errno.ENOSPC, 'Injected partial write')
os.write = fail_write
`;
    expect(await edit(home, true, '/mnt/c/bin', false, injection)).toEqual({ errors: ['.bashrc', '.profile'], updated: 0 });
    expect(await readdir(home)).toEqual([]);
  });
  it('handles successful short writes before publishing the complete replacement', async () => {
    const home = await fixture();
    await writeFile(path.join(home, '.bashrc'), '# keep me');
    const injection = `
import os
real_write = os.write
os.write = lambda fd, data: real_write(fd, data[:7])
`;
    expect(await edit(home, true, '/mnt/c/bin', false, injection)).toEqual({ errors: [], updated: 2 });
    expect(await edit(home, true, '/mnt/c/bin', false)).toEqual({ errors: [], updated: 0 });
    expect((await readdir(home)).sort()).toEqual(['.bashrc', '.profile']);
    await edit(home, false, '', false);
    expect(await readFile(path.join(home, '.bashrc'), 'utf8')).toBe('# keep me');
  });
  it.skipIf(process.platform === 'win32')('refuses replacement if the original identity changes during staging', async () => {
    const home = await fixture();
    await writeFile(path.join(home, '.bashrc'), '# original');
    const injection = `
import os, sys
from pathlib import Path
real_fsync = os.fsync
changed = False
def change_original(fd):
    global changed
    real_fsync(fd)
    if not changed:
        changed = True
        home = Path(sys.argv[3])
        replacement = home / 'user-replacement'
        replacement.write_bytes(b'# concurrently replaced by user')
        os.replace(replacement, home / '.bashrc')
os.fsync = change_original
`;
    expect(await edit(home, true, '/mnt/c/bin', false, injection)).toEqual({ errors: ['.bashrc'], updated: 1 });
    expect(await readFile(path.join(home, '.bashrc'), 'utf8')).toBe('# concurrently replaced by user');
    expect((await readdir(home)).sort()).toEqual(['.bashrc', '.profile']);
  });
  for (const exists of [true, false]) {
    it(`preserves a concurrent user ${exists ? 'edit' : 'creation'} instead of publishing the staged file`, async () => {
      const home = await fixture();
      if (exists) await writeFile(path.join(home, '.bashrc'), '# original');
      const injection = `
import os, sys
from pathlib import Path
real_fsync = os.fsync
changed = False
def change_original(fd):
    global changed
    real_fsync(fd)
    if not changed:
        changed = True
        (Path(sys.argv[3]) / '.bashrc').write_bytes(b'# concurrent user config')
os.fsync = change_original
`;
      expect(await edit(home, true, '/mnt/c/bin', false, injection)).toEqual({ errors: ['.bashrc'], updated: 1 });
      expect(await readFile(path.join(home, '.bashrc'), 'utf8')).toBe('# concurrent user config');
      expect((await readdir(home)).sort()).toEqual(['.bashrc', '.profile']);
    });
  }
  it('keeps preexisting empty files and user additions to files it created', async () => {
    const home = await fixture();
    await writeFile(path.join(home, '.bashrc'), '');
    await edit(home, true);
    const content = await readFile(path.join(home, '.zshrc'), 'utf8');
    await writeFile(path.join(home, '.zshrc'), content + '# added later\n');
    await edit(home, false);
    expect(await readFile(path.join(home, '.bashrc'), 'utf8')).toBe('');
    expect(await readFile(path.join(home, '.zshrc'), 'utf8')).toBe('# added later\n');
  });
  it('refuses malformed blocks and non-regular startup paths without clobbering them', async () => {
    const home = await fixture();
    const malformed = '# user\n# >>> Shellfox/wsl-cli-v1 >>>\nkeep this';
    await writeFile(path.join(home, '.bashrc'), malformed);
    await mkdir(path.join(home, '.profile'));
    expect((await edit(home, true)).errors).toEqual(['.bashrc', '.profile']);
    expect((await edit(home, false)).errors).toEqual(['.bashrc', '.profile']);
    expect(await readFile(path.join(home, '.bashrc'), 'utf8')).toBe(malformed);
  });
  it.skipIf(process.platform === 'win32')('refuses symlinks and hard links', async () => {
    const home = await fixture();
    const target = path.join(home, 'foreign');
    await writeFile(target, '# foreign');
    await symlink(target, path.join(home, '.bashrc'));
    await link(target, path.join(home, '.profile'));
    expect((await edit(home, true)).errors).toEqual(['.bashrc', '.profile']);
    expect(await readFile(target, 'utf8')).toBe('# foreign');
  });
});
