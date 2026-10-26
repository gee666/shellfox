import { constants } from 'node:fs';
import { lstat, open } from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import path from 'node:path';
import type { Readable } from 'node:stream';
import { CliUsageError } from './arguments';
const run = promisify(execFile);
export async function readStdin(input: Readable, limit = 1024 * 1024): Promise<string> {
  const chunks: Buffer[] = []; let length = 0;
  for await (const value of input) {
    const chunk = Buffer.isBuffer(value) ? value : Buffer.from(String(value));
    length += chunk.length;
    if (length > limit) throw new CliUsageError('Stdin exceeds the input limit.');
    chunks.push(chunk);
  }
  return new TextDecoder('utf-8', { fatal: true }).decode(Buffer.concat(chunks));
}
export async function readAuthKey(file: string): Promise<string> {
  const before = await lstat(file);
  if (before.isSymbolicLink() || !before.isFile() || before.nlink !== 1) throw new CliUsageError('Auth key must be a protected regular file, not a link.');
  if (process.platform === 'win32') {
    const script = `$ErrorActionPreference='Stop'; $p='${path.resolve(file).replaceAll("'", "''")}';
$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value;
$acl=[IO.File]::GetAccessControl($p);
if ($acl.GetOwner([Security.Principal.SecurityIdentifier]).Value -ne $sid) { throw 'Unsafe auth key owner' }
foreach ($rule in $acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier])) {
  if ($rule.AccessControlType -eq 'Allow' -and $rule.IdentityReference.Value -notin @($sid,'S-1-5-18','S-1-5-32-544')) { throw 'Auth key file is accessible to other users' }
}`;
    try {
      await run(path.win32.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/WindowsPowerShell/v1.0/powershell.exe'),
        ['-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(script, 'utf16le').toString('base64')], { timeout: 8000, windowsHide: true, maxBuffer: 16384 });
    } catch { throw new CliUsageError('Auth key file must be owned by the current user and readable only by that user, SYSTEM, or Administrators.'); }
  } else if (before.uid !== process.getuid?.() || (before.mode & 0o077)) throw new CliUsageError('Auth key file must be owned by the current user with mode 0600 or stricter.');
  const handle = await open(file, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
  try {
    const info = await handle.stat();
    if (info.dev !== before.dev || info.ino !== before.ino || !info.isFile() || info.nlink !== 1 || info.size > 4096 ||
      (process.platform !== 'win32' && (info.uid !== process.getuid?.() || (info.mode & 0o077)))) throw new CliUsageError('Unsafe auth key file.');
    const buffer = Buffer.alloc(4097);
    const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0);
    if (bytesRead > 4096) throw new CliUsageError('Auth key exceeds the input limit.');
    return validateAuthKey(new TextDecoder('utf-8', { fatal: true }).decode(buffer.subarray(0, bytesRead)));
  } finally { await handle.close(); }
}
export function validateAuthKey(value: string): string {
  const key = value.trim();
  if (!key || key.length > 4096 || /[\s\x00-\x1f\x7f]/.test(key)) throw new CliUsageError('Invalid auth key input.');
  return key;
}
