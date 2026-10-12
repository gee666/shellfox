import { afterEach, expect, it, vi } from 'vitest';
import path from 'node:path';
import { runCli, defaultUserData } from './cli';
import { runControlCli } from '../control-cli';
import { SshProfileStore } from './storage';
import { connectSsh } from './client';
vi.mock('../control-cli', () => ({ runControlCli: vi.fn(async () => 7) }));
vi.mock('./terminal', () => ({ CliTerminal: class {
 color = false; tty = false; start() {} restore() {} pick() { throw new Error('not expected'); }
}, Cancelled: class extends Error {} }));
vi.mock('./storage', () => ({ SshProfileStore: class { read = vi.fn(async () => []); } }));
vi.mock('./client', () => ({ connectSsh: vi.fn(async () => 0), SshConnectionError: class extends Error {} }));
afterEach(() => { vi.restoreAllMocks(); vi.clearAllMocks(); vi.unstubAllGlobals(); });
it('routes controls, aliases, help and version with exact user data to the control dispatcher', async () => {
 vi.stubGlobal('__SHELLFOX_VERSION__', '1.2.3');
 for (const args of [['ls'], ['proc', '--pid', '42'], ['tab', 'new'], ['keepalive'], ['-h'], ['help', 'ssh'], ['--version']]) {
  expect(await runCli(['--user-data', 'tmp/cli-data', ...args])).toBe(7);
  expect(runControlCli).toHaveBeenLastCalledWith(args, path.resolve('tmp/cli-data'), '1.2.3', { color: false, opensApp: process.platform !== 'win32' });
 }
 expect(connectSsh).not.toHaveBeenCalled();
});
it('retains direct SSH help and invalid-argument handling without control requests', async () => {
 const output = vi.spyOn(process.stdout, 'write').mockReturnValue(true);
 for (const flag of ['--help', '-h', 'help']) expect(await runCli(['ssh', flag])).toBe(0);
 expect(await runCli(['ssh', '--invalid'])).toBe(2);
 expect(output.mock.calls.map(c => c[0]).join('')).toContain('connect to a saved ssh connection');
 expect(runControlCli).not.toHaveBeenCalled(); expect(connectSsh).not.toHaveBeenCalled();
});
it('retains the standalone SSH no-profiles path', async () => {
 vi.spyOn(process.stdout, 'write').mockReturnValue(true);
 expect(await runCli(['ssh'])).toBe(2);
 expect(await runCli(['ssh', '--profile', 'prod'])).toBe(1);
 expect(runControlCli).not.toHaveBeenCalled(); expect(connectSsh).not.toHaveBeenCalled();
 expect(new SshProfileStore('/data').read).toBeDefined();
});
it('uses platform-specific default user data directories', () => {
 expect(defaultUserData('win32', { APPDATA: 'C:\\App Data' }, 'C:\\home')).toBe('C:\\App Data\\Shellfox');
 expect(defaultUserData('darwin', {}, '/home')).toBe('/home/Library/Application Support/Shellfox');
 expect(defaultUserData('linux', { XDG_CONFIG_HOME: '/config' }, '/home')).toBe('/config/Shellfox');
});
