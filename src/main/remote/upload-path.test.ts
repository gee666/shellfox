import { expect, it, vi } from 'vitest';
import type { TerminalProfileDto } from '../../shared/contracts';
import { terminalUploadPath } from './upload-path';

const local: TerminalProfileDto = { id: 'local', label: 'local', environment: 'local', executable: '/bin/bash', args: [], distro: null, available: true };
const guest: TerminalProfileDto = { ...local, id: 'wsl:Ubuntu', environment: 'wsl', distro: 'Ubuntu', executable: 'C:\\Windows\\System32\\wsl.exe' };
it('returns absolute native paths unchanged on Windows, macOS and Linux', async () => {
  for (const [platform, file] of [['win32', 'C:\\Users\\me\\uploads\\a.txt'], ['darwin', '/Users/me/uploads/a.txt'], ['linux', '/home/me/uploads/a.txt']] as const) {
    const run = vi.fn(); expect(await terminalUploadPath(file, local, platform, run)).toBe(file); expect(run).not.toHaveBeenCalled();
  }
});
it('translates into the exact selected WSL distro using argument boundaries, never shell text or PID guesses', async () => {
  const file = 'C:\\Uploads\\a file.txt', distro = 'Ubuntu; not-a-command', run = vi.fn(async () => '/mnt/c/Uploads/a file.txt\n');
  expect(await terminalUploadPath(file, { ...guest, distro }, 'win32', run)).toBe('/mnt/c/Uploads/a file.txt');
  expect(run).toHaveBeenCalledExactlyOnceWith(guest.executable, ['--distribution', distro, '--exec', '/usr/bin/wslpath', '-a', '-u', file]);
});
it('fails closed on invalid profiles, unavailable translation and malformed guest output', async () => {
  for (const profile of [{ ...guest, distro: null }, { ...guest, distro: 'bad\ndistro' }, { ...guest, executable: 'wsl.exe' }, { ...guest, executable: 'C:\\evil.exe' }, { ...guest, elevation: 'administrator' as const }, { ...guest, available: false }]) {
    const run = vi.fn(async () => '/mnt/c/x'); await expect(terminalUploadPath('C:\\x', profile, 'win32', run)).rejects.toThrow(); expect(run).not.toHaveBeenCalled();
  }
  await expect(terminalUploadPath('C:\\x', guest, 'linux')).rejects.toThrow();
  for (const output of ['relative', '/mnt/c/x\ncommand', '/mnt/c/x\0', '']) await expect(terminalUploadPath('C:\\x', guest, 'win32', async () => output)).rejects.toThrow();
  await expect(terminalUploadPath('C:\\x', guest, 'win32', async () => { throw new Error('WSL unavailable'); })).rejects.toThrow();
});
