import { expect, it, vi } from 'vitest';
import { linuxDirectory, guestDirectory, darwinDirectory, GUEST_DIRECTORY_SCRIPT, type LinuxDirectoryReader } from './current-directory';
import type { TrackingRoot } from './tracking';
const root: TrackingRoot = { tabId: 'tab', sessionId: 'session', generation: 'generation', pid: 17, environment: 'local', distro: null, marker: 'private-marker' };
const identity = { pid: 17, birth: 'boot:42' };
const stat = (ticks: string) => '17 (shell with spaces) ' + [...Array.from({ length: 19 }, () => '0'), ticks].join(' ');
function reader(after = '42', marked = true): LinuxDirectoryReader {
  let calls = 0;
  return { text: async file => file.endsWith('boot_id') ? 'boot\n' : stat(++calls === 1 ? '42' : after), marked: async () => marked, link: async () => '/work/after cd' };
}
it('reads current Linux process cwd only between matching birth and marker checks', async () => {
  expect(await linuxDirectory(root, identity, reader())).toBe('/work/after cd');
  await expect(linuxDirectory(root, identity, reader('43'))).rejects.toThrow('identity changed');
  await expect(linuxDirectory(root, identity, reader('42', false))).rejects.toThrow('identity changed');
  await expect(linuxDirectory(root, { ...identity, birth: 'other:42' }, reader())).rejects.toThrow('identity changed');
});
it('queries WSL through its exact distro and authenticated guest identity, never interpolating paths into source', async () => {
  const run = vi.fn(async () => Buffer.from(JSON.stringify("/home/user/a';& folder")));
  const guest = { ...root, environment: 'wsl' as const, distro: "Ubuntu space';&" }, id = { pid: 123, birth: '12345678-1234-1234-1234-123456789abc:42' };
  expect(await guestDirectory(guest, id, run)).toBe("/home/user/a';& folder");
  expect(run).toHaveBeenCalledWith(expect.stringMatching(/wsl\.exe$/), ['--distribution', guest.distro, '--exec', 'python3', '-c', GUEST_DIRECTORY_SCRIPT, '123', id.birth, root.marker]);
  expect(GUEST_DIRECTORY_SCRIPT).not.toContain(guest.distro); expect(GUEST_DIRECTORY_SCRIPT).toContain('assert birth()==ticks');
  await expect(guestDirectory(guest, id, async () => Buffer.from('"relative"'))).rejects.toThrow();
});
it('queries macOS cwd between authenticated supervisor identity checks and rejects multiple directory records', async () => {
  const verify = vi.fn(async () => {}), run = vi.fn(async () => Buffer.from('p17\0fcwd\0n/Users/me/current folder\0\n'));
  expect(await darwinDirectory(identity, verify, run)).toBe('/Users/me/current folder');
  expect(verify).toHaveBeenCalledTimes(2);
  expect(run).toHaveBeenCalledWith('/usr/sbin/lsof', ['-a', '-p', '17', '-d', 'cwd', '-F0fn']);
  const changed = vi.fn(async () => { if (changed.mock.calls.length === 2) throw new Error('identity changed'); });
  await expect(darwinDirectory(identity, changed, run)).rejects.toThrow('identity changed');
  await expect(darwinDirectory(identity, verify, async () => Buffer.from('n/first\0n/other\0'))).rejects.toThrow('could not be determined');
});
