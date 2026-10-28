import { execFile } from 'node:child_process';
import path from 'node:path';
import type { TerminalProfileDto } from '../../shared/contracts';
import { pathSchema } from '../../shared/schemas';

type Run = (file: string, args: string[]) => Promise<string>;
const run: Run = (file, args) => new Promise((resolve, reject) => {
  execFile(file, args, { encoding: 'utf8', windowsHide: true, timeout: 8000, maxBuffer: 32768 }, (error, stdout) => error ? reject(error) : resolve(stdout));
});

/** Only a discovered, selected terminal profile supplies the executable and distro. */
export async function terminalUploadPath(hostPath: string, profile: TerminalProfileDto, platform = process.platform, execute: Run = run): Promise<string> {
  if (!pathSchema.safeParse(hostPath).success || profile.elevation || !profile.available) throw new Error('Upload destination is unavailable.');
  if (profile.environment === 'local') return hostPath;
  if (platform !== 'win32' || !profile.distro || profile.distro.length > 160 || /[\x00-\x1f\x7f]/.test(profile.distro)
    || path.win32.basename(profile.executable).toLowerCase() !== 'wsl.exe' || !path.win32.isAbsolute(profile.executable)) throw new Error('Invalid WSL terminal profile.');
  const guest = (await execute(profile.executable, ['--distribution', profile.distro, '--exec', '/usr/bin/wslpath', '-a', '-u', hostPath])).replace(/\r?\n$/, '');
  if (!guest.startsWith('/') || !pathSchema.safeParse(guest).success || /[\r\n]/.test(guest)) throw new Error('WSL upload path could not be resolved.');
  return guest;
}
