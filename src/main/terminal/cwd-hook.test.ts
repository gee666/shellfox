import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import path from 'node:path';
import { afterEach, expect, it } from 'vitest';
import { powershellDirectoryHook } from './cwd-hook';
import { spawnArguments } from './profiles';
import type { TerminalProfileDto } from '../../shared/contracts';
const exec = promisify(execFile), directories: string[] = [];
afterEach(async () => { for (const directory of directories.splice(0)) await rm(directory, { recursive: true, force: true }); });
const profile: TerminalProfileDto = { id: 'windows-powershell', label: 'PowerShell', environment: 'local', executable: 'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe', args: ['-NoLogo'], distro: null, available: true };
it('installs directory reporting independently of CLI completion after loading the normal user profile', () => {
  for (const completion of [false, true]) {
    const launch = spawnArguments(profile, 'C:\\work', 'marker', completion), index = launch.args.indexOf('-EncodedCommand');
    expect(index).toBeGreaterThan(0); expect(launch.args).toContain('-NoExit'); expect(launch.args).not.toContain('-NoProfile');
    const script = Buffer.from(launch.args[index + 1]!, 'base64').toString('utf16le');
    expect(script).toContain('ShellfoxOriginalPrompt'); expect(script.includes('Register-ArgumentCompleter')).toBe(completion);
    expect(script).not.toContain('Set-Location');
  }
});
it.runIf(process.platform === 'win32')('reports actual PowerShell cd with escaped URI while retaining the loaded prompt and LASTEXITCODE', async () => {
  await mkdir('tmp', { recursive: true }); const directory = await mkdtemp(path.resolve('tmp/powershell-cwd-')); directories.push(directory);
  const target = path.join(directory, "changed Ω # % & ' folder"); await mkdir(target);
  const quote = (value: string) => "'" + value.replaceAll("'", "''") + "'";
  const script = `$ProgressPreference = 'SilentlyContinue'; function global:prompt { 'custom-user-prompt> ' }; $global:LASTEXITCODE = 23; Set-StrictMode -Version Latest; ${powershellDirectoryHook()}; Set-Location -LiteralPath ${quote(target)}; $rendered = prompt; [Console]::WriteLine('PROMPT=' + $rendered); [Console]::WriteLine('EXIT=' + $global:LASTEXITCODE)`;
  const executable = path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe');
  const { stdout, stderr } = await exec(executable, ['-NoLogo', '-NoProfile', '-EncodedCommand', Buffer.from(script, 'utf16le').toString('base64')], { timeout: 15000, maxBuffer: 65536, encoding: 'utf8', windowsHide: true });
  expect(stderr).toBe(''); expect(stdout).toContain('PROMPT=custom-user-prompt> '); expect(stdout).toContain('EXIT=23');
  const report = /\x1b\]7;([^\x07]+)\x07/.exec(stdout); expect(report).not.toBeNull();
  const url = new URL(report![1]!); expect(decodeURIComponent(url.pathname).slice(1).replace(/\//g, '\\').toLowerCase()).toBe(target.toLowerCase());
  expect(url.href).toContain('%23'); expect(url.href).toContain('%25');
});
