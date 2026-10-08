import { beforeEach, expect, it, vi } from 'vitest';

const io = vi.hoisted(() => ({ readdir: vi.fn(), stat: vi.fn() }));
vi.mock('node:fs/promises', () => io);
vi.mock('node:os', () => ({ homedir: () => process.platform === 'win32' ? 'C:\\Users\\Tester' : '/home/tester' }));
vi.mock('./directory', () => ({ validateDirectory: vi.fn() }));
import { completeDirectory } from './directory-prompt';

beforeEach(() => { io.readdir.mockReset(); io.stat.mockReset(); });

it.each([
  ['ENOENT', 'NOT_FOUND', 'does not exist'],
  ['ENOTDIR', 'NOT_FOUND', 'not a directory'],
  ['EACCES', 'VALIDATION', 'not accessible'],
  ['EPERM', 'VALIDATION', 'not accessible'],
  ['EIO', 'INTERNAL', 'Could not read'],
])('reports a clear completion failure for %s', async (code, resultCode, message) => {
  io.readdir.mockRejectedValueOnce(Object.assign(new Error('filesystem failed'), { code }));
  expect(await completeDirectory({ path: 'folder/' })).toMatchObject({ ok: false, error: { code: resultCode, message: expect.stringContaining(message) } });
});

it.each(['ENOENT', 'ENOTDIR'])('ignores dangling directory links with %s', async code => {
  io.readdir.mockResolvedValueOnce([{ name: 'link', isDirectory: () => false, isSymbolicLink: () => true }]);
  io.stat.mockRejectedValueOnce(Object.assign(new Error('gone'), { code }));
  expect(await completeDirectory({ path: '' })).toEqual({ ok: true, value: { matches: [] } });
});

it('does not disguise permission failures while checking links as an empty success', async () => {
  io.readdir.mockResolvedValueOnce([{ name: 'link', isDirectory: () => false, isSymbolicLink: () => true }]);
  io.stat.mockRejectedValueOnce(Object.assign(new Error('denied'), { code: 'EACCES' }));
  expect(await completeDirectory({ path: '' })).toMatchObject({ ok: false, error: { code: 'VALIDATION', message: expect.stringContaining('not accessible') } });
});
