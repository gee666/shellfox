import { execFile } from 'node:child_process';
import path from 'node:path';

export type WslCliRunner = (file: string, args: string[]) => Promise<Buffer>;
export interface WslCliOptions {
  run?: WslCliRunner;
  /** Explicit guest directory for isolated tests. Omitted means the distro's default user's HOME. */
  home?: string;
}
export const runWslCli: WslCliRunner = (file, args) => new Promise((resolve, reject) => {
  execFile(file, args, { encoding: 'buffer', windowsHide: true, timeout: 8000, maxBuffer: 256 * 1024 },
    (error, stdout) => error ? reject(error) : resolve(stdout));
});
const decode = (output: Buffer) => (output.includes(0) || output[0] === 0xff && output[1] === 0xfe ? output.toString('utf16le') : output.toString('utf8')).replace(/^\uFEFF/, '').replace(/\r/g, '');

// No startup shell is executed while editing files. All user values arrive as argv, never as code.
// Bytes outside our blocks are kept exactly, including a missing final newline. Refuse links,
// non-regular files, other owners, hard links, and malformed markers rather than guessing ownership.
export const WSL_STARTUP_SCRIPT = String.raw`
import json, os, re, shutil, stat, sys, tempfile
from pathlib import Path

installed = sys.argv[1] == 'enable'
bin_dir = sys.argv[2]
home = Path(sys.argv[3]) if sys.argv[3] else Path.home()
if not home.is_absolute() or not home.is_dir() or home.is_symlink():
    raise ValueError('Expected an existing, non-symlink home directory')
if installed and (not bin_dir.startswith('/') or any(c in bin_dir for c in '\r\n\0:')):
    raise ValueError('Invalid guest PATH entry')

begin = b'# >>> Shellfox/wsl-cli-v1 >>>\n'
end = b'# <<< Shellfox/wsl-cli-v1 <<<\n'
pattern = re.compile(rb'\n' + re.escape(begin) + rb'# Created by Shellfox: (yes|no)\n(.*?)' + re.escape(end), re.S)
def quote(value):
    return "'" + value.replace("'", "'\\''") + "'"
def block(created):
    q = quote(bin_dir)
    body = ('# Windows launcher before Linux launchers.\n'
            'if [ -f ' + q + '/shellfox ]; then\n'
            '  case "$' + '{PATH-}" in\n'
            '    ' + q + '|' + q + ':*) ;;\n'
            '    *) export PATH=' + q + ':"$' + '{PATH-}" ;;\n'
            '  esac\n'
            '  hash -r 2>/dev/null || :\n'
            'fi\n').encode('utf-8')
    return b'\n' + begin + b'# Created by Shellfox: ' + (b'yes' if created else b'no') + b'\n' + body + end

def identity(info):
    # Windows Python reports different ctime values for lstat/fstat. Guest Linux
    # checks ctime too; the native Windows test editor still checks mode and mtime.
    return (info.st_dev, info.st_ino, info.st_mode, info.st_nlink, info.st_uid,
            info.st_gid, info.st_size, info.st_mtime_ns, info.st_ctime_ns if os.name != 'nt' else None)
def check_original(file, fd, opened, content):
    if identity(file.lstat()) != identity(opened) or identity(os.fstat(fd)) != identity(opened):
        raise ValueError('File identity, ownership or metadata changed')
    os.lseek(fd, 0, os.SEEK_SET)
    with os.fdopen(os.dup(fd), 'rb') as stream:
        if stream.read() != content:
            raise ValueError('File content changed')
    if identity(file.lstat()) != identity(opened) or identity(os.fstat(fd)) != identity(opened):
        raise ValueError('File changed while checking content')

all_names = ['.bashrc', '.bash_profile', '.bash_login', '.profile', '.zshrc', '.zprofile', '.zlogin']
targets = set()
if installed:
    if shutil.which('bash'):
        targets.add('.bashrc')
        # Bash reads only the first existing login file. Do not create .bash_profile
        # over an existing .profile, which would hide unrelated login configuration.
        targets.add(next((n for n in ['.bash_profile', '.bash_login', '.profile'] if os.path.lexists(home / n)), '.profile'))
    if shutil.which('zsh') or any(os.path.lexists(home / n) for n in ['.zshrc', '.zprofile', '.zlogin']):
        targets.update(['.zshrc', '.zprofile', '.zlogin'])
errors = ['No supported shell found'] if installed and not targets else []
updated = 0
for name in all_names:
    file = home / name
    fd = None
    staged_fd = None
    staged = None
    try:
        exists = os.path.lexists(file)
        if not exists and name not in targets:
            continue
        if exists:
            info = file.lstat()
            if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or (hasattr(os, 'getuid') and info.st_uid != os.getuid()):
                raise ValueError('Not a regular file owned by the current user')
            flags = os.O_RDONLY | getattr(os, 'O_NOFOLLOW', 0) | getattr(os, 'O_NONBLOCK', 0) | getattr(os, 'O_BINARY', 0)
            fd = os.open(file, flags)
            opened = os.fstat(fd)
            if identity(opened) != identity(info):
                raise ValueError('File changed while opening')
            with os.fdopen(os.dup(fd), 'rb') as stream:
                content = stream.read()
        else:
            content = b''
        matches = list(pattern.finditer(content))
        if content.count(b'# >>> Shellfox/wsl-cli-v1 >>>') != len(matches) or content.count(b'# <<< Shellfox/wsl-cli-v1 <<<') != len(matches) or len(matches) > 1:
            raise ValueError('Malformed Shellfox block')
        created = not exists or bool(matches and matches[0].group(1) == b'yes')
        remaining = pattern.sub(b'', content)
        # Remove our old block from inactive login files too, if the user changed
        # which file Bash reads. Never remove any unmarked configuration.
        desired = remaining + block(created) if name in targets else remaining
        if desired == content:
            continue
        if not desired and created:
            if fd is not None:
                check_original(file, fd, opened, content)
                os.close(fd)
                fd = None
                file.unlink()
        else:
            # Never write into a user's startup file. A short write, ENOSPC, or
            # failed fsync leaves only an incomplete stage that finally removes.
            staged_fd, staged = tempfile.mkstemp(prefix=name + '.shellfox-', dir=home)
            staged_info = os.fstat(staged_fd)
            data = memoryview(desired)
            while data:
                written = os.write(staged_fd, data)
                if written <= 0:
                    raise OSError('Short write while staging startup configuration')
                data = data[written:]
            if exists and hasattr(os, 'fchown'):
                os.fchown(staged_fd, opened.st_uid, opened.st_gid)
            mode = stat.S_IMODE(opened.st_mode) if exists else 0o600
            if hasattr(os, 'fchmod'):
                os.fchmod(staged_fd, mode)
            else:
                os.chmod(staged, mode)
            os.fsync(staged_fd)
            current_stage = os.lstat(staged)
            if (current_stage.st_dev, current_stage.st_ino) != (staged_info.st_dev, staged_info.st_ino) or not stat.S_ISREG(current_stage.st_mode) or current_stage.st_nlink != 1:
                raise ValueError('Staged file changed')
            if exists:
                check_original(file, fd, opened, content)
                os.close(fd)
                fd = None
            os.close(staged_fd)
            staged_fd = None
            if exists:
                os.replace(staged, file)
                staged = None
            else:
                # Publish atomically without replacing a concurrently created file.
                os.link(staged, file)
                os.unlink(staged)
                staged = None
        updated += 1
    except Exception:
        errors.append(name)
    finally:
        if fd is not None:
            os.close(fd)
        if staged_fd is not None:
            os.close(staged_fd)
        if staged is not None:
            try:
                os.unlink(staged)
            except FileNotFoundError:
                pass
            except OSError:
                if name not in errors:
                    errors.append(name)
print(json.dumps({'errors': errors, 'updated': updated}))
`;

/** Best-effort Windows opt-in. A broken or absent guest must not disable the Windows command. */
export async function setWslCli(installed: boolean, binDir: string, options: WslCliOptions = {}): Promise<string | null> {
  if (options.home !== undefined && (!options.home.startsWith('/') || /[\r\n\0]/.test(options.home))) {
    return 'WSL startup configuration skipped: invalid isolated guest home.';
  }
  const run = options.run ?? runWslCli;
  const wsl = path.win32.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'wsl.exe');
  let distros: string[];
  try {
    distros = [...new Set(decode(await run(wsl, ['--list', '--quiet'])).split('\n').map(s => s.trim()).filter(Boolean))];
    if (distros.length > 32 || distros.some(s => s.length > 160 || /[\u0000-\u001f\u007f]/.test(s))) throw new Error('Invalid distro list');
  } catch { return 'WSL startup configuration unavailable. Windows CLI is unaffected.'; }
  const failed: string[] = [];
  // Bounded concurrency and per-process timeouts, including distributions that need to boot.
  for (let offset = 0; offset < distros.length; offset += 4) {
    await Promise.all(distros.slice(offset, offset + 4).map(async distro => {
      try {
        const prefix = ['--distribution', distro, '--exec'];
        const guestBin = installed ? decode(await run(wsl, [...prefix, 'wslpath', '-a', '-u', binDir])).trimEnd() : '';
        if (installed && (!guestBin.startsWith('/') || /[\r\n\0:]/.test(guestBin))) throw new Error('Invalid guest bin');
        const result: unknown = JSON.parse(decode(await run(wsl, [...prefix, '/usr/bin/env', 'python3', '-c', WSL_STARTUP_SCRIPT, installed ? 'enable' : 'disable', guestBin, options.home ?? ''])));
        if (!result || typeof result !== 'object' || !('errors' in result) || !Array.isArray(result.errors) || result.errors.length ||
          !('updated' in result) || !Number.isInteger(result.updated) || (result.updated as number) < 0) throw new Error('Startup files skipped');
      } catch { failed.push(distro); }
    }));
  }
  if (failed.length) {
    const prefix = 'WSL startup configuration incomplete for ';
    const suffix = '. Windows CLI is unaffected; restart WSL shells after retrying.';
    const names = failed.join(', ');
    const budget = 1000 - prefix.length - suffix.length;
    return prefix + (names.length > budget ? names.slice(0, budget - 3) + '...' : names) + suffix;
  }
  return distros.length ? 'Restart WSL shells to apply the change. Bash: hash -r; zsh: rehash after reloading startup files.' : null;
}
