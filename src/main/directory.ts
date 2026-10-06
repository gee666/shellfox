import { access, realpath, stat } from 'node:fs/promises';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { pathSchema, isWslUncPath } from '../shared/schemas';
const exec = promisify(execFile);
export async function validateDirectory(input: string): Promise<string> {
  pathSchema.parse(input);
  if (process.platform === 'win32' && !isWslUncPath(input) && (!/^[A-Za-z]:[\\/]/.test(input) || input.slice(2).includes(':'))) throw new Error('Expected a local drive directory');
  const canonical = await realpath(input);
  pathSchema.parse(canonical);
  if (!(await stat(canonical)).isDirectory()) throw new Error('Not a directory');
  await access(canonical);
  if (process.platform === 'win32' && !isWslUncPath(canonical)) {
    // Only the validated drive letter enters this fixed script, never the folder.
    const drive = canonical.slice(0, 1).toUpperCase();
    if (!/^[A-Z]$/.test(drive)) throw new Error('Remote paths are unsupported');
    const executable = path.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe');
    const script = "[Console]::Write([System.IO.DriveInfo]::new('" + drive + ":\\').DriveType.ToString())";
    const { stdout } = await exec(executable, ['-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(script, 'utf16le').toString('base64')], { windowsHide: true, timeout: 5000, maxBuffer: 4096 });
    if (!['Fixed', 'Removable', 'Ram'].includes(stdout.trim())) throw new Error('Only local drives are supported');
  }
  return path.normalize(input);
}
