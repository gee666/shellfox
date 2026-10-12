import path from 'node:path';
import { constants } from 'node:fs';
import { chmod, lstat, mkdir, open, rename, unlink } from 'node:fs/promises';
import { randomBytes } from 'node:crypto';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { controlEndpointPath, controlEndpointSchema } from './control-protocol';

const run = promisify(execFile);
async function windowsPermissions(file: string, directory: boolean, protect: boolean): Promise<void> {
  const literal = "'" + file.replace(/'/g, "''") + "'";
  const script = `$ErrorActionPreference='Stop'; $p=${literal}; $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User;
${protect ? `$acl=[IO.${directory ? 'Directory' : 'File'}]::GetAccessControl($p,[Security.AccessControl.AccessControlSections]::Access);
$acl.SetAccessRuleProtection($true,$false);
foreach ($old in $acl.GetAccessRules($true,$false,[Security.Principal.SecurityIdentifier])) { $acl.RemoveAccessRuleSpecific($old) }
$rule=New-Object Security.AccessControl.FileSystemAccessRule($sid,'FullControl'${directory ? ",'ContainerInherit,ObjectInherit','None'" : ''},'Allow');
$acl.AddAccessRule($rule); [IO.${directory ? 'Directory' : 'File'}]::SetAccessControl($p,$acl);` : ''}
$acl=[IO.${directory ? 'Directory' : 'File'}]::GetAccessControl($p);
if (!$acl.AreAccessRulesProtected -or $acl.GetOwner([Security.Principal.SecurityIdentifier]).Value -ne $sid.Value) { throw 'Unsafe control endpoint owner or inheritance' }
$rules=$acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier]);
if ($rules.Count -eq 0) { throw 'Missing control endpoint permissions' }
foreach ($r in $rules) { if ($r.IdentityReference.Value -ne $sid.Value -or $r.AccessControlType -ne 'Allow') { throw 'Unsafe control endpoint permissions' } }`;
  await run(path.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe'), ['-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(script, 'utf16le').toString('base64')], { windowsHide: true, timeout: 8000, maxBuffer: 16384 });
}
async function checkPrivate(file: string, directory: boolean): Promise<void> {
  const info = await lstat(file);
  if (info.isSymbolicLink() || (directory ? !info.isDirectory() : !info.isFile() || info.nlink !== 1)) throw new Error('Unsafe control endpoint file.');
  if (process.platform === 'win32') await windowsPermissions(file, directory, false);
  else if (info.uid !== process.getuid?.() || (info.mode & (directory ? 0o077 : 0o177)) !== 0) throw new Error('Unsafe control endpoint permissions.');
}
export async function publishControlEndpoint(userData: string, endpoint: unknown): Promise<() => Promise<void>> {
  const data = controlEndpointSchema.parse(endpoint);
  const file = controlEndpointPath(userData), directory = path.dirname(file);
  await mkdir(userData, { recursive: true, mode: 0o700 });
  if (process.platform !== 'win32') {
    const parent = await lstat(userData);
    if (!parent.isDirectory() || parent.isSymbolicLink() || parent.uid !== process.getuid?.() || (parent.mode & 0o022)) throw new Error('Unsafe user data directory for control transport.');
  }
  try { await mkdir(directory, { mode: 0o700 }); }
  catch (error) { if ((error as NodeJS.ErrnoException).code !== 'EEXIST') throw error; await checkPrivate(directory, true); }
  if (process.platform === 'win32') await windowsPermissions(directory, true, true);
  else await chmod(directory, 0o700);
  await checkPrivate(directory, true);
  const staged = path.join(directory, randomBytes(16).toString('hex') + '.json');
  const handle = await open(staged, 'wx', 0o600);
  try {
    if (process.platform === 'win32') await windowsPermissions(staged, false, true);
    await handle.writeFile(JSON.stringify(data), 'utf8');
    await handle.sync(); await handle.close();
    await rename(staged, file);
  } catch (error) { await handle.close().catch(() => {}); await unlink(staged).catch(() => {}); throw error; }
  return async () => {
    // An old instance must never unlink a newer instance's credentials.
    try { if ((await readControlEndpoint(userData)).secret === data.secret) await unlink(file); }
    catch (error) { if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error; }
  };
}
export async function readControlEndpoint(userData: string) {
  const file = controlEndpointPath(userData);
  await checkPrivate(path.dirname(file), true); await checkPrivate(file, false);
  const handle = await open(file, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
  try {
    const info = await handle.stat();
    if (!info.isFile() || info.size > 4096 || info.nlink !== 1) throw new Error('Invalid control endpoint file.');
    if (process.platform !== 'win32' && (info.uid !== process.getuid?.() || (info.mode & 0o177))) throw new Error('Unsafe control endpoint permissions.');
    const buffer = Buffer.alloc(4097);
    const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0);
    if (bytesRead > 4096) throw new Error('Invalid control endpoint file.');
    return controlEndpointSchema.parse(JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(buffer.subarray(0, bytesRead))));
  } finally { await handle.close(); }
}
