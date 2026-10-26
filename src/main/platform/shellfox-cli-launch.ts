import path from 'node:path';
import { access, stat } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import { z } from 'zod';
import { CliUsageError } from '../../cli/arguments';

export interface WindowsLaunchConfig { executable: string; args: string[]; env: Record<string, string> }
export function launchConfig(executable: string, args: string[], env: Record<string, string>): string {
  return Buffer.from(JSON.stringify({ executable, args, env } satisfies WindowsLaunchConfig)).toString('base64');
}
/** Windows cmd is only an argv forwarding shim. It never interpolates arguments into tests or start. */
export async function dispatchWindowsLaunch(args: string[]): Promise<number | null> {
  if (process.env.SHELLFOX_HEADLESS === '1' || !process.env.SHELLFOX_CLI_LAUNCH) return null;
  const command = (args[0] ?? '').toLowerCase();
  if (args.slice(1).some(arg => ['--help', '-h', 'help'].includes(arg))) return null;
  if (!['start', 'add', 'new', 'create', 'update'].includes(command)) return null;
  const config = z.object({ executable: z.string().min(1), args: z.array(z.string()), env: z.record(z.string(), z.string()) }).strict()
    .parse(JSON.parse(Buffer.from(process.env.SHELLFOX_CLI_LAUNCH, 'base64').toString('utf8')));
  if (command === 'update') {
    if (process.platform !== 'win32') return null;
    if (args.length > 2 || (args[1] && args[1] !== '--check')) {
      process.stderr.write('Usage: shellfox update [--check]\n'); return 2;
    }
    const updateScript = process.env.SHELLFOX_CLI_UPDATE;
    if (!updateScript) throw new Error('Shellfox update script is missing. Reinstall CLI integration.');
    const extra = args[1] === '--check' ? ['-Check'] : [];
    const executable = path.win32.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe');
    return new Promise((resolve, reject) => {
      const child = spawn(executable, ['-NoLogo', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', updateScript, '-Executable', config.executable, ...extra], { stdio: 'inherit', shell: false });
      child.once('error', reject); child.once('close', code => resolve(code ?? 1));
    });
  }
  if (args.length > 2) throw new CliUsageError('Usage: shellfox start [path]');
  const folder = path.resolve(args[1] || '.');
  if (!(await stat(folder)).isDirectory()) throw new Error('Directory not found: ' + folder);
  await access(config.executable);
  const env = { ...process.env, ...config.env }; delete env.ELECTRON_RUN_AS_NODE;
  delete env.SHELLFOX_CLI_LAUNCH; delete env.SHELLFOX_CLI_UPDATE;
  return new Promise((resolve, reject) => {
    const child = spawn(config.executable, [...config.args, 'start', folder], { cwd: process.cwd(), env, detached: true, stdio: 'ignore', shell: false });
    child.once('error', reject);
    child.once('spawn', () => { child.unref(); process.stdout.write('Shellfox: started session in ' + JSON.stringify(folder) + '\n'); resolve(0); });
  });
}
