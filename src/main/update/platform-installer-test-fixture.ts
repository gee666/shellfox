import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { expect, vi } from 'vitest';
import { createUpdatePlatform } from './platform-installer';

const state = vi.hoisted(() => ({ calls: [] as { file: string; args: string[]; control: string }[] }));
vi.mock('./handoff', async importOriginal => ({ ...await importOriginal<typeof import('./handoff')>(), startHandoff: vi.fn(async (file: string, args: string[], control: string) => {
  state.calls.push({ file, args, control });
  return { committed: false, commit: vi.fn(async () => {}), cancel: vi.fn(async () => {}) };
}) }));

type ParseFixture = (file: string, script: string, config: string, asset: string) => Promise<unknown>;
export async function assertWindowsHandoffs(parse?: ParseFixture): Promise<void> {
  state.calls = [];
  await mkdir('tmp', { recursive: true }); const root = await mkdtemp(path.resolve('tmp/updater-platform-'));
  try {
    const app = path.join(root, 'app-0.2.5'); await mkdir(app);
    await writeFile(path.join(root, 'Update.exe'), 'fake');
    const platform = await createUpdatePlatform({ platform: 'win32', arch: 'x64', executable: path.join(app, 'Shellfox.exe'), packaged: true, pid: 123 });
    expect(platform.supported).toBe(true); expect(platform.assetName('0.2.6')).toBe('ShellfoxSetup.exe');
    const work = path.join(root, 'work Café & 雪'); await mkdir(work); const file = path.join(work, 'ShellfoxSetup.exe'); await writeFile(file, 'fake');
    const start = await platform.prepare(file, '0.2.6', work); await start(); await start(false);
    expect(state.calls[0]!.control).not.toBe(state.calls[1]!.control);
    expect(state.calls[0]!.args).toContain(path.join(work, 'install.ps1'));
    const config = JSON.parse(await readFile(path.join(state.calls[0]!.control, 'install.json'), 'utf8'));
    expect(config).toMatchObject({ pid: 123, file, work, updateExe: path.join(root, 'Update.exe'), control: state.calls[0]!.control, relaunch: true }); expect(config.sha256).toMatch(/^[a-f0-9]{64}$/);
    const quitConfig = JSON.parse(await readFile(path.join(state.calls[1]!.control, 'install.json'), 'utf8'));
    expect(quitConfig.relaunch).toBe(false);
    expect(await readdir(work)).toHaveLength(4);
    const script = await readFile(path.join(work, 'install.ps1'), 'utf8');
    expect(script.indexOf("'committed'")).toBeLessThan(script.indexOf('Wait-Process'));
    expect(script.indexOf('Wait-Process')).toBeLessThan(script.indexOf('Start-Process'));
    expect(script).toContain('Get-FileHash'); expect(script).toContain('-LiteralPath'); expect(script).toContain('-Raw -Encoding UTF8');
    expect(script).toContain('--processStart Shellfox.exe'); expect(script.match(/if \(\$c\.relaunch\)/g)).toHaveLength(2); expect(script).toContain('if (-not $managerExited)');
    await parse?.(state.calls[0]!.file, path.join(work, 'install.ps1'), path.join(state.calls[0]!.control, 'install.json'), file);
  } finally { await rm(root, { recursive: true, force: true }); }
}
