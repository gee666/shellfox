import { execFile } from 'node:child_process';
import path from 'node:path';

export type PermissionCommand = (file: string, args: string[], input?: string) => Promise<string>;
const execute: PermissionCommand = (file, args, input) => new Promise((resolve, reject) => {
  const child = execFile(file, args, { encoding: 'utf8', timeout: 8000, windowsHide: true, maxBuffer: 32768 }, (error, stdout) => error ? reject(error) : resolve(stdout));
  child.stdin?.on('error', () => {}); child.stdin?.end(input);
});

export function parseWindowsUserSid(output: string): string {
  const match = /^"(?:[^"\r\n]|"")*","(S-1-\d+(?:-\d+){1,15})"$/.exec(output.trim());
  const sid = match?.[1], parts = sid?.split('-').slice(2).map(Number);
  if (!sid || !parts || parts[0]! > 281474976710655 || parts.slice(1).some(n => !Number.isSafeInteger(n) || n > 4294967295)) throw new Error('Could not identify the upload directory owner.');
  return sid;
}

// Construct the entire protected DACL in memory, then apply it once. Never inherit
// the parent's ACL, even briefly. Paths and SIDs arrive as JSON data, not code.
const ownerOnlyDacl = `
$ErrorActionPreference = 'Stop'
$data = ConvertFrom-Json -InputObject ([Console]::In.ReadToEnd())
$directory = [System.IO.DirectoryInfo]::new([string]$data.directory)
if (($directory.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Upload directory is a reparse point.' }
$sid = [System.Security.Principal.SecurityIdentifier]::new([string]$data.sid)
if ($sid.Value -ne [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value) { throw 'Upload owner identity changed.' }
$current = [System.IO.Directory]::GetAccessControl($directory.FullName, [System.Security.AccessControl.AccessControlSections]::Owner)
if ($current.GetOwner([System.Security.Principal.SecurityIdentifier]).Value -ne $sid.Value) { throw 'Upload directory has a different owner.' }
$security = [System.Security.AccessControl.DirectorySecurity]::new()
$security.SetAccessRuleProtection($true, $false)
$inheritance = [System.Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [System.Security.AccessControl.InheritanceFlags]::ObjectInherit
$rule = [System.Security.AccessControl.FileSystemAccessRule]::new($sid, [System.Security.AccessControl.FileSystemRights]::FullControl, $inheritance, [System.Security.AccessControl.PropagationFlags]::None, [System.Security.AccessControl.AccessControlType]::Allow)
$security.AddAccessRule($rule)
[System.IO.Directory]::SetAccessControl($directory.FullName, $security)
`;

export async function protectWindowsUploadDirectory(directory: string, run: PermissionCommand = execute): Promise<void> {
  const system = path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32');
  const sid = parseWindowsUserSid(await run(path.join(system, 'whoami.exe'), ['/user', '/fo', 'csv', '/nh']));
  const powershell = path.join(system, 'WindowsPowerShell', 'v1.0', 'powershell.exe');
  await run(powershell, ['-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(ownerOnlyDacl, 'utf16le').toString('base64')], JSON.stringify({ directory, sid }));
}
