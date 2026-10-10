import { afterEach, expect, it, vi } from 'vitest';
import path from 'node:path';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { WindowsCliIntegration, shellfoxShims } from './shellfox-cli';
import * as wslCli from './wsl-cli';
import * as explorer from './explorer';
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
    expect(await cli.set(true)).toMatchObject({ ok: true, value: { installed: true, command: 'shellfox' } });
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

const isolatedDirectories: string[] = [];
afterEach(async () => {
  vi.restoreAllMocks(); vi.unstubAllEnvs();
  await Promise.all(isolatedDirectories.splice(0).map(dir => rm(dir, { recursive: true, force: true })));
});
async function isolatedBin() {
  await mkdir(path.resolve('tmp'), { recursive: true });
  const directory = await mkdtemp(path.resolve('tmp/shellfox-wsl-wiring-'));
  isolatedDirectories.push(directory);
  return directory;
}
function isolateLocalAppData(directory: string) {
  vi.stubEnv('LOCALAPPDATA', directory);
  if (process.platform !== 'win32') {
    // Keep default options for the opt-in test, but map its Windows bin to a
    // native absolute path. Backslashes would otherwise name a repo-relative file.
    const join = path.win32.join;
    vi.spyOn(path.win32, 'join').mockImplementation((...parts) =>
      parts.length === 3 && parts[0] === directory && parts[1] === 'Shellfox' && parts[2] === 'bin'
        ? path.join(...parts) : join(...parts));
  }
}
function registryRunner() {
  let installed = false;
  return vi.fn(async (script: string) => {
    const data = JSON.parse(Buffer.from(/FromBase64String\('([^']+)'\)/.exec(script)![1], 'base64').toString());
    if (data.installed !== null) installed = data.installed;
    return { ok: true, value: { installed } };
  });
}
it('uses one PATH subprocess per write and skips PATH reads for stale shims', async () => {
  const binDir = await isolatedBin(), run = registryRunner();
  const cli = new WindowsCliIntegration({ executable: process.execPath, platform: 'win32', binDir, run, wsl: false });
  expect(await cli.set(true)).toMatchObject({ ok: true, value: { installed: true } });
  expect(run).toHaveBeenCalledTimes(1);
  await writeFile(path.join(binDir, 'shellfox.cmd'), 'stale');
  expect(await cli.get()).toMatchObject({ ok: true, value: { installed: false } });
  expect(run).toHaveBeenCalledTimes(1);
  expect(await cli.set(true)).toMatchObject({ ok: true, value: { installed: true } });
  expect(run).toHaveBeenCalledTimes(2);
  expect(await cli.get()).toMatchObject({ ok: true, value: { installed: true } });
  expect(run).toHaveBeenCalledTimes(3);
  expect(await cli.set(false)).toMatchObject({ ok: true, value: { installed: false } });
  expect(run).toHaveBeenCalledTimes(4);
});
it('does not start PowerShell to check PATH when CLI files are missing', async () => {
  const binDir = await isolatedBin(), run = registryRunner();
  const cli = new WindowsCliIntegration({ executable: process.execPath, platform: 'win32', binDir, run, wsl: false });
  expect(await cli.get()).toMatchObject({ ok: true, value: { installed: false } });
  expect(run).not.toHaveBeenCalled();
});
it('rechecks shim contents after WSL work without a second PATH subprocess', async () => {
  const binDir = await isolatedBin(), run = registryRunner();
  vi.spyOn(wslCli, 'setWslCli').mockImplementation(async () => {
    await writeFile(path.join(binDir, 'shellfox-owner.json'), 'foreign');
    return 'Restart WSL shells.';
  });
  const cli = new WindowsCliIntegration({ executable: process.execPath, platform: 'win32', binDir, run, wsl: {} });
  expect(await cli.set(true)).toMatchObject({ ok: true, value: { installed: false, reason: 'Restart WSL shells.' } });
  expect(run).toHaveBeenCalledOnce();
});
it('uses the returned PATH state, not an assumed successful install', async () => {
  const binDir = await isolatedBin(), run = vi.fn(async () => ({ ok: true, value: { installed: false } }));
  const cli = new WindowsCliIntegration({ executable: process.execPath, platform: 'win32', binDir, run, wsl: false });
  expect(await cli.set(true)).toMatchObject({ ok: true, value: { installed: false } });
  expect(run).toHaveBeenCalledOnce();
});
it('production defaults invoke WSL on enable and disable, but never on a read', async () => {
  const directory = await isolatedBin();
  isolateLocalAppData(directory);
  vi.spyOn(explorer, 'runRegistry').mockImplementation(registryRunner());
  const wsl = vi.spyOn(wslCli, 'setWslCli').mockResolvedValue('Restart WSL shells.');
  const cli = new WindowsCliIntegration({ executable: process.execPath, platform: 'win32' });
  expect(path.relative(directory, cli.binDir)).toBe(path.join('Shellfox', 'bin'));
  await cli.get(); expect(wsl).not.toHaveBeenCalled();
  expect(await cli.set(true)).toMatchObject({ ok: true, value: { installed: true, reason: 'Restart WSL shells.' } });
  expect(wsl).toHaveBeenLastCalledWith(true, cli.binDir, {});
  expect(await cli.set(false)).toMatchObject({ ok: true, value: { installed: false } });
  expect(wsl).toHaveBeenLastCalledWith(false, cli.binDir, {});
});
it('custom registry/bin/runner options and an explicit opt-out cannot touch real WSL HOME', async () => {
  const directory = await isolatedBin();
  isolateLocalAppData(directory);
  vi.spyOn(explorer, 'runRegistry').mockImplementation(registryRunner());
  const wsl = vi.spyOn(wslCli, 'setWslCli').mockResolvedValue(null);
  for (const extra of [{ binDir: directory }, { registryKey: 'Software\\ShellfoxTests\\Isolated' }, { run: registryRunner() }, { wsl: false as const }]) {
    const cli = new WindowsCliIntegration({ executable: process.execPath, platform: 'win32', ...extra });
    expect(path.relative(directory, cli.binDir)).toBe('binDir' in extra ? '' : path.join('Shellfox', 'bin'));
    expect(await cli.set(true)).toMatchObject({ ok: true, value: { installed: true } });
    expect(await cli.set(false)).toMatchObject({ ok: true, value: { installed: false } });
  }
  expect(wsl).not.toHaveBeenCalled();
});
it('explicit isolated WSL opt-in uses its runner and does not fail Windows when WSL is unavailable', async () => {
  const binDir = await isolatedBin();
  const run = vi.fn(async () => { throw new Error('WSL unavailable'); });
  const cli = new WindowsCliIntegration({ executable: process.execPath, platform: 'win32', binDir, run: registryRunner(), wsl: { run, home: '/tmp/shellfox-isolated-home' } });
  expect(await cli.set(true)).toMatchObject({ ok: true, value: { installed: true, reason: expect.stringContaining('Windows CLI is unaffected') } });
  expect(await readFile(path.join(binDir, 'shellfox'), 'utf8')).toContain('Shellfox/cli-v1');
  expect(run).toHaveBeenCalledTimes(1);
  expect(await cli.set(false)).toMatchObject({ ok: true, value: { installed: false } });
  expect(run).toHaveBeenCalledTimes(2);
});
it('does not invoke WSL on non-Windows platforms or after a registry failure', async () => {
  const binDir = await isolatedBin();
  const wsl = vi.spyOn(wslCli, 'setWslCli').mockResolvedValue(null);
  const run = vi.fn(async () => { throw new Error('Registry failure'); });
  const linux = new WindowsCliIntegration({ executable: process.execPath, platform: 'linux', binDir, run, wsl: {} });
  expect(await linux.set(true)).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
  expect(run).not.toHaveBeenCalled();
  const windows = new WindowsCliIntegration({ executable: process.execPath, platform: 'win32', binDir, run, wsl: {} });
  expect(await windows.set(true)).toMatchObject({ ok: false, error: { code: 'NATIVE_UNAVAILABLE' } });
  expect(wsl).not.toHaveBeenCalled();
});
