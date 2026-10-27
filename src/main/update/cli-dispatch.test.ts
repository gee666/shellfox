import { ChildProcess } from 'node:child_process';
import * as childProcess from 'node:child_process';
import path from 'node:path';
import { afterEach, expect, it, vi } from 'vitest';
import { dispatchDesktopLaunch, launchConfig } from '../platform/shellfox-cli-launch';
import updateSh from './shellfox-update.sh';

vi.mock('node:child_process', async importOriginal => ({ ...await importOriginal<typeof import('node:child_process')>(), spawn: vi.fn() }));
afterEach(() => { vi.restoreAllMocks(); vi.resetAllMocks(); vi.unstubAllEnvs(); vi.unstubAllGlobals(); });
function configure(platform: string) {
  vi.stubGlobal('process', { ...process, platform });
  vi.stubEnv('SHELLFOX_HEADLESS', '');
  vi.stubEnv('SHELLFOX_CLI_LAUNCH', launchConfig('/installed app/Shellfox', [], {}));
  vi.stubEnv('SHELLFOX_CLI_UPDATE', 'C:\\installed app\\shellfox-update.ps1');
  vi.stubEnv('ELECTRON_RUN_AS_NODE', '1');
  vi.mocked(childProcess.spawn).mockImplementation(() => {
    const child = new ChildProcess();
    queueMicrotask(() => child.emit('close', 23, null));
    return child;
  });
}
it.each(['linux', 'darwin'])('dispatches canonical %s updates with literal argv and the bound app executable', async platform => {
  configure(platform);
  for (const args of [['update'], ['update', '--check']]) {
    expect(await dispatchDesktopLaunch(args)).toBe(23);
    expect(childProcess.spawn).toHaveBeenLastCalledWith('/bin/sh', ['-c', updateSh, 'shellfox-update', ...args.slice(1)], {
      stdio: 'inherit', shell: false, env: expect.objectContaining({ SHELLFOX_APP_EXECUTABLE: '/installed app/Shellfox' }),
    });
    const options = vi.mocked(childProcess.spawn).mock.calls.at(-1)?.[2];
    expect(options?.env).not.toHaveProperty('ELECTRON_RUN_AS_NODE');
  }
});
it('keeps Windows PowerShell routing and its installer exit status', async () => {
  configure('win32');
  expect(await dispatchDesktopLaunch(['update', '--check'])).toBe(23);
  expect(childProcess.spawn).toHaveBeenCalledWith(
    path.win32.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/WindowsPowerShell/v1.0/powershell.exe'),
    ['-NoLogo', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', 'C:\\installed app\\shellfox-update.ps1', '-Executable', '/installed app/Shellfox', '-Check'],
    { stdio: 'inherit', shell: false },
  );
});
it.each(['linux', 'darwin', 'win32'])('never dispatches help, invalid options, or headless installs as a %s desktop update', async platform => {
  configure(platform);
  vi.spyOn(process.stderr, 'write').mockImplementation(() => true);
  expect(await dispatchDesktopLaunch(['update', '--help'])).toBeNull();
  expect(await dispatchDesktopLaunch(['update', '--check', '--check'])).toBe(2);
  expect(await dispatchDesktopLaunch(['update', '--json'])).toBe(2);
  vi.stubEnv('SHELLFOX_HEADLESS', '1');
  expect(await dispatchDesktopLaunch(['update'])).toBeNull();
  expect(childProcess.spawn).not.toHaveBeenCalled();
});
it('propagates updater spawn failure rather than claiming an install', async () => {
  configure('linux');
  vi.mocked(childProcess.spawn).mockImplementation(() => {
    const child = new ChildProcess(); queueMicrotask(() => child.emit('error', new Error('sh failed'))); return child;
  });
  await expect(dispatchDesktopLaunch(['update'])).rejects.toThrow('sh failed');
});
