import { execFile } from 'node:child_process';
import { access, realpath } from 'node:fs/promises';
import { constants } from 'node:fs';
import { userInfo } from 'node:os';
import path from 'node:path';
import type { TerminalProfileDto, TerminalProfilesDto } from '../../shared/contracts';
import { pathSchema, terminalProfileSchema } from '../../shared/schemas';
import { parseWslUnc } from '../../shared/wsl-path';
import { validateDirectory } from '../directory';
import { PIDFD_PREFLIGHT, PIDFD_READY } from './pidfd-preflight';
import { resolvePython, type PythonProbe } from './python';
import { getDarwinSupervisorCapability } from './darwin-supervisor';
import type { TrackingHelperOptions } from './tracking';

export type Execute = (file: string, args: string[]) => Promise<Buffer>;
export const execute: Execute = (file, args) => new Promise((resolve, reject) => {
  execFile(file, args, { encoding: 'buffer', windowsHide: true, timeout: 8000, maxBuffer: 1024 * 1024 }, (error, stdout) => error ? reject(error) : resolve(stdout));
});
export function decodeWsl(output: Buffer): string {
  return (output.includes(0) ? output.toString('utf16le') : output.toString('utf8')).replace(/^\uFEFF/, '').replace(/\r/g, '');
}
export interface ProfileOptions {
  platform?: NodeJS.Platform; env?: NodeJS.ProcessEnv;
  exists?: (file: string) => Promise<boolean>; run?: Execute; loginShell?: string | null;
  canonical?: (file: string) => Promise<string>;
  supervisorOptions?: TrackingHelperOptions; pythonPath?: string | null; python?: PythonProbe;
}
export async function discoverProfiles(options: ProfileOptions = {}): Promise<TerminalProfilesDto> {
  const platform = options.platform ?? process.platform, env = options.env ?? process.env, run = options.run ?? execute;
  const exists = options.exists ?? (async file => { try { await access(file, platform === 'win32' ? constants.F_OK : constants.X_OK); return true; } catch { return false; } });
  const profiles: TerminalProfileDto[] = [];
  if (platform === 'win32') {
    const candidates = [
      { id: 'pwsh', label: 'PowerShell', executable: path.win32.join(env.ProgramFiles || 'C:\\Program Files', 'PowerShell', '7', 'pwsh.exe') },
      { id: 'windows-powershell', label: 'Windows PowerShell', executable: path.win32.join(env.SystemRoot || 'C:\\Windows', 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe') },
    ];
    for (const p of candidates) if (await exists(p.executable)) profiles.push({ ...p, environment: 'local', args: ['-NoLogo'], distro: null, available: true, canTerminateDescendants: true, unavailableReason: null });
    const wsl = path.win32.join(env.SystemRoot || 'C:\\Windows', 'System32', 'wsl.exe');
    if (await exists(wsl)) {
      try {
        const distros = [...new Set(decodeWsl(await run(wsl, ['--list', '--quiet'])).split('\n').map(s => s.trim()).filter(s => s && s.length <= 160 && !/[\u0000-\u001f\u007f]/.test(s)))].slice(0, 32);
        // A distro name is not proof of a usable shell or safe close support. Preflight before permitting launch.
        const probeScript = `test -x /bin/bash && test -x /usr/bin/env && command -v python3 >/dev/null && exec python3 -c "$1"`;
        const probed: TerminalProfileDto[] = [];
        for (let offset = 0; offset < distros.length; offset += 4) {
          const batch = await Promise.all(distros.slice(offset, offset + 4).map(async distro => {
            let available = false;
            try { available = decodeWsl(await run(wsl, ['--distribution', distro, '--exec', '/bin/sh', '-c', probeScript, 'shellfox-preflight', PIDFD_PREFLIGHT])).trim() === PIDFD_READY; } catch { /* A failed preflight is an unavailable profile, not a launch permission. */ }
            return { id: `wsl:${distro}`, label: `WSL · ${distro}`.slice(0, 200), environment: 'wsl' as const, executable: wsl, args: ['--distribution', distro, '--exec', '/bin/bash', '-l', '-i'], distro, available, canTerminateDescendants: available, unavailableReason: available ? null : 'Requires accessible Bash, env, Python3, /proc and working Linux pidfd signal support. Guest preflight failed; no shell may be launched.' };
          }));
          probed.push(...batch);
        }
        profiles.push(...probed);
      } catch { /* WSL missing, unconfigured or unavailable is not a discovered shell. */ }
    }
  } else if (platform === 'linux' || platform === 'darwin') {
    let login = options.loginShell;
    if (login === undefined) { try { login = userInfo().shell; } catch { login = null; } }
    const candidates = [...new Set([login, env.SHELL, platform === 'darwin' ? '/bin/zsh' : '/bin/bash', '/bin/sh'].filter((p): p is string => !!p && pathSchema.safeParse(p).success))];
    let closeReady = false, supervisorReason: string | null = null;
    const python = platform === 'linux' ? options.python ?? await resolvePython(options.pythonPath ?? null, { env, exists, run }) : null;
    if (platform === 'darwin') { const supervisor = await getDarwinSupervisorCapability({ ...options.supervisorOptions, platform }, run); closeReady = supervisor.available; supervisorReason = supervisor.reason; }
    if (platform === 'linux') closeReady = !!python?.usable;
    for (const original of candidates) if (await exists(original)) {
      let executable = original, canonicalReady = false;
      try { executable = await (options.canonical ?? realpath)(original); canonicalReady = pathSchema.safeParse(executable).success && await exists(executable); } catch { /* Canonical identity could not be verified. */ }
      const available = canonicalReady && closeReady;
      profiles.push({ id: original === candidates[0] ? 'login-shell' : `shell:${original}`, label: `${path.posix.basename(original)} login shell`, environment: 'local', executable, args: ['-l', '-i'], distro: null, available, canTerminateDescendants: available, unavailableReason: !canonicalReady ? 'The canonical shell executable could not be verified.' : platform === 'darwin' && !closeReady ? `macOS supervisor preflight unavailable: ${supervisorReason ?? 'Bundled native helper is unavailable.'}` : !closeReady ? python?.reason ?? 'Python 3 not found. Set its path in Settings → Python.' : null });
    }
  }
  return { profiles: profiles.filter(p => terminalProfileSchema.safeParse(p).success), defaultProfileId: profiles.find(p => p.available)?.id ?? null, lifetime: 'app-owned', shellSurvival: false };
}
export async function validateProfileCwd(profile: TerminalProfileDto, cwd: string, run: Execute = execute): Promise<string> {
  pathSchema.parse(cwd);
  if (profile.environment === 'local') return validateDirectory(cwd);
  if (!profile.distro) throw new Error('No guest distribution selected.');
  let guest = cwd;
  const unc = parseWslUnc(cwd);
  if (unc) {
    if (unc.distro.toLowerCase() !== profile.distro.toLowerCase()) throw new Error('The folder belongs to a different WSL distribution.');
    guest = unc.guestPath;
  }
  if (/^[A-Za-z]:[\\/]/.test(cwd)) guest = decodeWsl(await run(profile.executable, ['--distribution', profile.distro, '--exec', 'wslpath', '-a', '-u', cwd])).trim();
  if (!guest.startsWith('/') || !pathSchema.safeParse(guest).success) throw new Error('Expected an absolute guest directory.');
  await run(profile.executable, ['--distribution', profile.distro, '--exec', '/bin/sh', '-c', 'test -d "$1" && test -x "$1"', 'shellfox-cwd', guest]);
  return guest;
}
export function spawnArguments(profile: TerminalProfileDto, cwd: string, marker: string): { file: string; args: string[]; cwd?: string } {
  if (profile.environment === 'local') return { file: profile.executable, args: [...profile.args], cwd };
  return { file: profile.executable, args: ['--distribution', profile.distro!, '--cd', cwd, '--exec', '/usr/bin/env', `SHELLFOX_TERMINAL_MARKER=${marker}`, '/bin/bash', '-l', '-i'] };
}
