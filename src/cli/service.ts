import path from 'node:path';
import { spawn } from 'node:child_process';
import { access } from 'node:fs/promises';
import { CommandArguments, CliUsageError, type GlobalOptions } from './arguments';
import { requireConfirmation } from './execute';
import { CliFailure, type TextOutput } from './output';
import { colorEnabled } from './human';
import { styles } from '../main/ssh/presentation';
/** Installer is shipped with the headless runtime. Never execute a caller-supplied script. */
export async function runHostService(args: string[], userData: string, options: GlobalOptions, output: TextOutput = process.stdout): Promise<unknown> {
  const [operation, ...rest] = args;
  const parsed = new CommandArguments(rest, [], ['--startup']); parsed.arity(0);
  if (operation !== 'install' && operation !== 'uninstall') throw new CliUsageError('Usage: shellfox host service install [--startup] | uninstall');
  if (parsed.has('--startup') && operation !== 'install') throw new CliUsageError('--startup only applies to installation.');
  if (options.host && options.host !== 'local') throw new CliUsageError('Service installation only manages the local host.');
  if (operation === 'uninstall') await requireConfirmation(options, 'Uninstall host autostart');
  const root = process.env.SHELLFOX_RUNTIME_PATH, resources = process.env.SHELLFOX_RESOURCES_PATH;
  if ((!root || !path.isAbsolute(root)) && (!resources || !path.isAbsolute(resources))) throw new CliFailure('DEPENDENCY_MISSING', 'The service installer is unavailable. Run the CLI installed with a desktop or headless package.');
  const script = root && path.isAbsolute(root) ? path.join(root, 'service', 'install.mjs') : path.join(resources ?? '', 'cli', 'service', 'install.mjs');
  const executable = process.execPath;
  const env = { ...process.env };
  if (process.versions.electron) env.ELECTRON_RUN_AS_NODE = '1'; else delete env.ELECTRON_RUN_AS_NODE;
  await access(script);
  const argv = [script, operation, '--user-data', path.resolve(userData), ...(parsed.has('--startup') ? ['--startup'] : [])];
  const result = await new Promise<string>((resolve, reject) => {
    const child = spawn(executable, argv, { stdio: ['ignore', 'pipe', 'pipe'], shell: false, windowsHide: true, timeout: 60000, env });
    let stdout = '', stderr = '', bytes = 0, exceeded = false;
    const capture = (data: Buffer, error: boolean) => {
      bytes += data.length;
      if (bytes > 1024 * 1024) { exceeded = true; child.kill(); return; }
      if (error) stderr += data.toString('utf8'); else stdout += data.toString('utf8');
    };
    child.stdout.on('data', data => capture(data, false)); child.stderr.on('data', data => capture(data, true));
    child.once('error', reject);
    child.once('close', code => {
      if (exceeded) reject(new CliFailure('INTERNAL', 'Service installer output exceeded its limit.'));
      else if (code !== 0) reject(new CliFailure('SERVICE_FAILED', stderr.trim() || stdout.trim() || 'Service installer failed.'));
      else resolve(stdout.trim());
    });
  });
  if (!options.json) output.write(styles(colorEnabled(output)).accent('Host service ' + operation) + '\n' + (result ? result + '\n' : 'Done.\n'));
  return { operation, message: result };
}
