import { execFile } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { constants } from 'node:fs';
import { access, readFile, readlink, realpath, writeFile } from 'node:fs/promises';
import path from 'node:path';

/**
 * Linux memory isolation for embedded terminals.
 *
 * Without it every shell shares the manager's systemd scope. When the kernel
 * OOM-kills one runaway command, the scope's default OOMPolicy=stop then
 * terminates the manager and every other terminal. Each shell instead gets a
 * transient user scope that survives OOM kills of its own descendants, and a
 * higher OOM score so the kernel prefers terminal work over the manager.
 */

/** Same preference Chromium gives renderers; the manager keeps its default score. */
export const TERMINAL_OOM_SCORE_ADJ = 300;

type Run = (file: string, args: string[]) => Promise<void>;
const run: Run = (file, args) => new Promise((resolve, reject) => {
  execFile(file, args, { timeout: 5000, windowsHide: true }, error => { if (error) reject(error); else resolve(); });
});
// The probe runs these exact options, so a systemd that rejects any of them
// (older releases cannot set OOMPolicy on scopes) falls back to direct shells.
const scopeArguments = () => ['--user', '--scope', '--quiet', '--collect', `--unit=shellfox-terminal-${randomUUID()}`, '--description=Shellfox terminal', '--property=OOMPolicy=continue', '--'];

/** Returns a systemd-run that can create user scopes, or null outside a systemd user session. */
export async function probeTerminalScope(searchPath = process.env.PATH ?? '', exec: Run = run): Promise<string | null> {
  for (const directory of [...searchPath.split(':'), '/usr/bin', '/bin']) {
    if (!path.isAbsolute(directory)) continue;
    const candidate = path.join(directory, 'systemd-run');
    try { await access(candidate, constants.X_OK); } catch { continue; }
    try { await exec(candidate, [...scopeArguments(), 'true']); return candidate; } catch { return null; }
  }
  return null;
}

/** systemd-run execs the command in place, so the PTY child PID stays the shell's PID. */
export function scopedCommand(runner: string, command: { file: string; args: string[]; cwd?: string }): { file: string; args: string[]; cwd?: string } {
  return { ...command, file: runner, args: [...scopeArguments(), command.file, ...command.args] };
}

/** Best effort: raise only, because unprivileged processes cannot lower their score again. */
export async function raiseOomScore(pid: number, value = TERMINAL_OOM_SCORE_ADJ): Promise<void> {
  const file = `/proc/${pid}/oom_score_adj`;
  try { if (Number((await readFile(file, 'utf8')).trim()) < value) await writeFile(file, String(value)); }
  catch { /* The shell may have exited, or the kernel lacks the interface. */ }
}

/** Shell identity checks need the final executable, not the PTY fork or the systemd-run wrapper. */
export async function waitForScopedExec(pid: number, runner: string, timeoutMs = 5000): Promise<void> {
  const wrappers = new Set([runner, process.execPath]);
  try { wrappers.add(await realpath(runner)); } catch { /* Compare the configured path only. */ }
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    let executable: string;
    try { executable = (await readlink(`/proc/${pid}/exe`)).replace(/ \(deleted\)$/, ''); } catch { return; }
    if (!wrappers.has(executable)) return;
    await new Promise(resolve => setTimeout(resolve, 5));
  }
}
