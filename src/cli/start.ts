import path from 'node:path';
import { execFile } from 'node:child_process';
import { realpath } from 'node:fs/promises';
import { promisify } from 'node:util';
import { CliUsageError } from './arguments';
const run = promisify(execFile);
export async function resolveStartDirectory(input = '.'): Promise<string> {
  if (!input || /[\0\r\n]/.test(input)) throw new CliUsageError('Invalid directory.');
  if (process.platform !== 'win32') return realpath(path.isAbsolute(input) ? input : process.cwd() + '/' + input);
  const distro = process.env.SHELLFOX_CLI_DISTRO;
  const windowsPath = /^[a-z]:[\\/]|^\\\\/i.test(input);
  if (distro && !windowsPath) {
    const cwd = process.env.SHELLFOX_CLI_WSL_CWD;
    if (!cwd || !path.posix.isAbsolute(cwd)) throw new CliUsageError('Missing WSL caller directory.');
    const executable = path.win32.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/wsl.exe');
    return (await run(executable, ['--distribution', distro, '--exec', 'wslpath', '-w', path.posix.resolve(cwd, input)], { timeout: 10000, maxBuffer: 65536 })).stdout.trim();
  }
  if (process.env.SHELLFOX_CLI_GIT_BASH && !windowsPath) {
    const executable = path.win32.join(path.win32.dirname(process.env.SHELLFOX_CLI_GIT_BASH), 'cygpath.exe');
    return (await run(executable, ['-wa', '--', input], { cwd: process.env.SHELLFOX_CLI_GIT_CWD, timeout: 10000, maxBuffer: 65536 })).stdout.trim();
  }
  return path.win32.resolve(input);
}
