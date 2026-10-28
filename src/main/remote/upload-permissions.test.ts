import { execFile } from 'node:child_process';
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import path from 'node:path';
import { promisify } from 'node:util';
import { afterEach, expect, it, vi } from 'vitest';
import { parseWindowsUserSid, protectWindowsUploadDirectory, type PermissionCommand } from './upload-permissions';

const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => { for (const cleanup of cleanups.splice(0).reverse()) await cleanup(); });
it('parses complete ordinary and cloud account SIDs without assuming an authority', () => {
  for (const sid of ['S-1-5-21-111-222-333-1001', 'S-1-12-1-111-222-333-444', 'S-1-281474976710655-4294967295']) {
    expect(parseWindowsUserSid(`"domain\\user","${sid}"\r\n`)).toBe(sid);
  }
  for (const output of ['S-1-5-21', '"user","S-1-12-1-111-222-333-444junk"', '"user","S-1-5-"', '"user","S-1-5"', '"user","S-2-5-1"', '"user","S-1-5--1"', '"user","S-1-5-4294967296"', '"user","S-1-281474976710656-1"', '"user","S-1-5-1"\n"other","S-1-5-2"', '"user","S-1-5-1" garbage']) {
    expect(() => parseWindowsUserSid(output), output).toThrow();
  }
});
it('passes paths as JSON, replaces the complete DACL once and never resets or inherits parent permissions', async () => {
  const directory = "C:\\public parent\\private'; Write-Output injected", sid = 'S-1-12-1-111-222-333-444';
  const run = vi.fn<PermissionCommand>().mockResolvedValueOnce(`"cloud\\user","${sid}"\r\n`).mockResolvedValueOnce('');
  await protectWindowsUploadDirectory(directory, run);
  expect(run).toHaveBeenCalledTimes(2);
  const [, args, input] = run.mock.calls[1]!;
  expect(JSON.parse(input ?? '')).toEqual({ directory, sid });
  expect(args.slice(0, 4)).toEqual(['-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand']);
  const script = Buffer.from(args[4]!, 'base64').toString('utf16le');
  expect(script).not.toContain(directory); expect(script).not.toContain(sid);
  expect(script).toContain('SetAccessRuleProtection($true, $false)');
  expect(script.match(/::SetAccessControl\(/g)).toHaveLength(1);
  expect(script).not.toMatch(/icacls|\/reset|SetAccessRuleProtection\(\$false/i);
});
it.runIf(process.platform === 'win32')('keeps a private root private under a readable parent, including when the DACL setter fails', async () => {
  await mkdir('tmp', { recursive: true }); const parent = await mkdtemp(path.resolve('tmp/upload-acl-'));
  cleanups.push(() => rm(parent, { recursive: true, force: true }));
  const directory = path.join(parent, "private' directory"), system = path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32');
  const native = promisify(execFile);
  const run: PermissionCommand = (file, args, input) => new Promise((resolve, reject) => {
    const child = execFile(file, args, { encoding: 'utf8', timeout: 8000, windowsHide: true }, (error, stdout) => error ? reject(error) : resolve(stdout));
    child.stdin?.on('error', () => {}); child.stdin?.end(input);
  });
  await native(path.join(system, 'icacls.exe'), [parent, '/grant', '*S-1-1-0:(OI)(CI)RX'], { timeout: 8000 });
  await mkdir(directory);
  const sid = parseWindowsUserSid(await run(path.join(system, 'whoami.exe'), ['/user', '/fo', 'csv', '/nh']));
  const permissions = async () => (await native(path.join(system, 'icacls.exe'), [directory], { timeout: 8000, encoding: 'utf8' })).stdout;
  await protectWindowsUploadDirectory(directory, run);
  const privateAcl = await permissions(); expect(privateAcl).not.toMatch(/Everyone|\(I\)/i);
  const observations: string[] = [];
  await protectWindowsUploadDirectory(directory, async (file, args, input) => {
    observations.push(await permissions()); return run(file, args, input);
  });
  for (const acl of observations) expect(acl).toBe(privateAcl);
  await expect(protectWindowsUploadDirectory(directory, async (file, args, input) => {
    expect(await permissions()).toBe(privateAcl);
    if (args.includes('-EncodedCommand')) throw new Error('Injected DACL setter failure');
    return run(file, args, input);
  })).rejects.toThrow('Injected DACL setter failure');
  expect(await permissions()).toBe(privateAcl);
  // Verify by SID too, independent of localized display names.
  const inspect = `$acl = [IO.Directory]::GetAccessControl([Console]::In.ReadToEnd()); if (!$acl.AreAccessRulesProtected) { throw 'Inherited ACL' }; $rules = $acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]); foreach ($rule in $rules) { if ($rule.IdentityReference.Value -ne '${sid}') { throw 'Unexpected access rule' } }; if ($rules.Count -ne 1) { throw 'Unexpected rule count' }`;
  await run(path.join(system, 'WindowsPowerShell', 'v1.0', 'powershell.exe'), ['-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(inspect, 'utf16le').toString('base64')], directory);
});
