import { expect, it } from 'vitest';
import path from 'node:path';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { WindowsCliIntegration, shellfoxShims } from './shellfox-cli';
const exec = promisify(execFile);
const ps = async (script: string) => (await exec('powershell.exe', ['-NoLogo','-NoProfile','-NonInteractive','-EncodedCommand',Buffer.from(script,'utf16le').toString('base64')], { timeout: 10000, maxBuffer: 65536 })).stdout;
it('generates immediate-return cmd and quoted POSIX/WSL launchers with usage and caller-directory resolution', () => {
  const scripts = shellfoxShims({ executable: 'C:\\Program Files\\Shellfox.exe', appPath: 'P:\\project space' });
  expect(scripts.cmd).toContain('start "" /D "%CD%" "C:\\Program Files\\Shellfox.exe" "P:\\project space" start');
  expect(scripts.cmd).toContain('setlocal DisableDelayedExpansion'); expect(scripts.cmd).not.toContain('/WAIT');
  expect(scripts.cmd).toContain('shellfox_folder=%~f2');
  expect(scripts.posix).toContain('wslpath -w "$(realpath "$folder")"');
  expect(scripts.posix).toContain("export MSYS_ARG_CONV_EXCL='*'");
  expect(scripts.posix).toContain('>/dev/null 2>&1 &'); // Git Bash stays detached.
  expect(scripts.posix).toContain('-File "$dispatcher" -Folder "$folder" >/dev/null || exit 1');
  expect(scripts.dispatcher).toContain("param([Parameter(Mandatory=$true)][string]$Folder)");
  expect(scripts.dispatcher).toContain('FromBase64String(');
  expect(scripts.dispatcher).toContain('Start-Process -FilePath $config.executable');
  expect(scripts.dispatcher).not.toContain('Invoke-Expression');
});
it.skipIf(process.platform !== 'win32')('writes owned shims, reflects files+PATH, preserves expandable PATH, deduplicates and removes only its entry', async () => {
  await mkdir(path.resolve('tmp'), { recursive: true });
  const directory = await mkdtemp(path.resolve('tmp/shellfox-cli-')), key = 'Software\\ShellfoxTests\\' + randomUUID() + '\\Environment';
  const binDir = path.join(directory, 'Shellfox bin');
  const cli = new WindowsCliIntegration({ executable: process.execPath, binDir, registryKey: key });
  try {
    await ps(`$k=[Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('${key}'); try { $k.SetValue('Path','%USERPROFILE%\\bin;;C:\\keep;',[Microsoft.Win32.RegistryValueKind]::ExpandString) } finally { $k.Dispose() }`);
    expect(await cli.get()).toMatchObject({ ok: true, value: { installed: false } });
    expect(await cli.set(true)).toMatchObject({ ok: true, value: { installed: true, command: 'shellfox start <path>' } });
    expect(await cli.set(true)).toMatchObject({ ok: true, value: { installed: true } });
    const state = JSON.parse(await ps(`$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('${key}'); try { @{ path=$k.GetValue('Path','',[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames); kind=$k.GetValueKind('Path').ToString() } | ConvertTo-Json -Compress } finally { $k.Dispose() }`));
    expect(state.kind).toBe('ExpandString'); expect(state.path).toBe('%USERPROFILE%\\bin;;C:\\keep;;' + binDir);
    await writeFile(path.join(binDir,'shellfox.cmd'), 'stale'); expect(await cli.get()).toMatchObject({ ok: true, value: { installed: false } });
    expect(await cli.set(true)).toMatchObject({ ok: true, value: { installed: true } });
    expect((await readFile(path.join(binDir,'shellfox.cmd'),'utf8')).includes('Shellfox: started')).toBe(true);
    expect(await cli.set(false)).toMatchObject({ ok: true, value: { installed: false } });
    expect((await ps(`$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('${key}'); try { $k.GetValue('Path','',[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) } finally { $k.Dispose() }`)).trim()).toBe('%USERPROFILE%\\bin;;C:\\keep;');
  } finally {
    await ps(`[Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree('${key.split('\\Environment')[0]}',$false)`);
    await rm(directory,{ recursive: true, force: true });
  }
}, 30000);
it('does not overwrite foreign shellfox files', async () => {
  await mkdir(path.resolve('tmp'), { recursive: true }); const binDir = await mkdtemp(path.resolve('tmp/shellfox-foreign-'));
  try {
    await writeFile(path.join(binDir,'shellfox.cmd'),'foreign');
    const cli = new WindowsCliIntegration({ executable: 'C:\\app.exe', platform: 'win32', binDir });
    expect(await cli.set(true)).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
    expect(await readFile(path.join(binDir,'shellfox.cmd'),'utf8')).toBe('foreign');
  } finally { await rm(binDir,{ recursive: true, force: true }); }
});
