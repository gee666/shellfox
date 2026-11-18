import { expect, it, vi } from 'vitest';
import { parseWindowsUserSid, protectWindowsUploadDirectory, type PermissionCommand } from './upload-permissions';

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
