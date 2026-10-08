import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { afterEach, expect, it, vi } from 'vitest';
const state = vi.hoisted(() => ({ calls: [] as { file: string; args: string[]; control: string }[] }));
vi.mock('./handoff', async importOriginal => ({ ...await importOriginal<typeof import('./handoff')>(), startHandoff: vi.fn(async (file: string, args: string[], control: string) => {
  state.calls.push({ file, args, control });
  return { committed: false, commit: vi.fn(async () => {}), cancel: vi.fn(async () => {}) };
}) }));
import { createUpdatePlatform } from './platform-installer';
afterEach(() => { state.calls = []; });
it.each(['win32', 'linux', 'darwin'])('disables self-installation in development on %s', async platform => {
  expect(await createUpdatePlatform({ platform, arch: 'x64', executable: '/fake/Shellfox', packaged: false })).toMatchObject({ supported: false });
});
it('does not try Windows x64 installers on ARM64 or portable ZIP copies', async () => {
  for (const input of [{ arch: 'arm64', executable: 'C:\\Shellfox\\app-0.2.5\\Shellfox.exe' }, { arch: 'x64', executable: 'C:\\Downloads\\Shellfox.exe' }]) {
    expect(await createUpdatePlatform({ ...input, platform: 'win32', packaged: true })).toMatchObject({ supported: false });
  }
});
it.each(['ia32', 'riscv64'])('rejects unpublished architecture %s', async arch => {
  expect(await createUpdatePlatform({ platform: 'linux', arch, executable: '/fake', packaged: true })).toMatchObject({ supported: false });
});
it('does not replace arbitrary macOS executables or read-only/unidentified bundles', async () => {
  for (const executable of ['/fake/Shellfox', '/fake/Shellfox.app/Contents/MacOS/Shellfox']) expect(await createUpdatePlatform({ platform: 'darwin', arch: 'arm64', executable, packaged: true })).toMatchObject({ supported: false });
});
it.skipIf(process.platform !== 'win32')('uses fresh handoff controls on every Windows attempt and only installs after commit and PID exit', async () => {
  await mkdir('tmp', { recursive: true }); const root = await mkdtemp(path.resolve('tmp/updater-platform-'));
  try {
    const app = path.join(root, 'app-0.2.5'); await mkdir(app);
    await writeFile(path.join(root, 'Update.exe'), 'fake');
    const platform = await createUpdatePlatform({ platform: 'win32', arch: 'x64', executable: path.join(app, 'Shellfox.exe'), packaged: true, pid: 123 });
    expect(platform.supported).toBe(true); expect(platform.assetName('0.2.6')).toBe('ShellfoxSetup.exe');
    const work = path.join(root, 'work Café & 雪'); await mkdir(work); const file = path.join(work, 'ShellfoxSetup.exe'); await writeFile(file, 'fake');
    const start = await platform.prepare(file, '0.2.6', work); await start(); await start();
    expect(state.calls[0]!.control).not.toBe(state.calls[1]!.control);
    expect(state.calls[0]!.args).toContain(path.join(work, 'install.ps1'));
    const config = JSON.parse(await readFile(path.join(state.calls[0]!.control, 'install.json'), 'utf8'));
    expect(config).toMatchObject({ pid: 123, file, work, updateExe: path.join(root, 'Update.exe'), control: state.calls[0]!.control }); expect(config.sha256).toMatch(/^[a-f0-9]{64}$/);
    expect(await readdir(work)).toHaveLength(4);
    const script = await readFile(path.join(work, 'install.ps1'), 'utf8');
    expect(script.indexOf("'committed'")).toBeLessThan(script.indexOf('Wait-Process'));
    expect(script.indexOf('Wait-Process')).toBeLessThan(script.indexOf('Start-Process'));
    expect(script).toContain('Get-FileHash'); expect(script).toContain('-LiteralPath'); expect(script).toContain('-Raw -Encoding UTF8');
    expect(script).toContain('--processStart Shellfox.exe'); expect(script).toContain('if (-not $managerExited)');
    // Parse with Windows PowerShell 5.1 without executing any installer code.
    await promisify(execFile)(state.calls[0]!.file, ['-NoProfile', '-NonInteractive', '-Command', "$tokens=$null;$errors=$null;[System.Management.Automation.Language.Parser]::ParseFile($env:SHELLFOX_PARSE_SCRIPT,[ref]$tokens,[ref]$errors)|Out-Null;if($errors.Count){$errors|Out-String|Write-Error;exit 1};$c=Get-Content -LiteralPath $env:SHELLFOX_PARSE_CONFIG -Raw -Encoding UTF8|ConvertFrom-Json;if($c.file -cne $env:SHELLFOX_PARSE_ASSET){exit 2}"], { timeout: 10_000, windowsHide: true, env: { ...process.env, SHELLFOX_PARSE_SCRIPT: path.join(work, 'install.ps1'), SHELLFOX_PARSE_CONFIG: path.join(state.calls[0]!.control, 'install.json'), SHELLFOX_PARSE_ASSET: file } });
  } finally { await rm(root, { recursive: true, force: true }); }
});
