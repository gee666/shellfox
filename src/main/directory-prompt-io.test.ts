import { beforeEach, expect, it, vi } from 'vitest';

const io = vi.hoisted(() => ({ readdir: vi.fn(), stat: vi.fn(), mkdir: vi.fn() }));
vi.mock('node:fs/promises', () => io);
vi.mock('node:os', () => ({ homedir: () => process.platform === 'win32' ? 'C:\\Users\\Tester' : '/home/tester' }));
vi.mock('./directory', () => ({ validateDirectory: vi.fn() }));
import { completeDirectory, resolveDirectory, createDirectory } from './directory-prompt';
import { validateDirectory } from './directory';

beforeEach(() => { io.readdir.mockReset(); io.stat.mockReset(); io.mkdir.mockReset(); vi.mocked(validateDirectory).mockReset(); });

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

it.each(['ENOTDIR', 'EACCES', 'EPERM'])('never reports %s as a missing directory or creates through it', async code => {
  vi.mocked(validateDirectory).mockRejectedValue(Object.assign(new Error('cannot resolve'), { code }));
  expect(await resolveDirectory({ path: '~/new' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await createDirectory({ path: '~/new' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(io.mkdir).not.toHaveBeenCalled();
});
it('reports only an actual missing filesystem path as NOT_FOUND', async () => {
  vi.mocked(validateDirectory).mockRejectedValue(Object.assign(new Error('missing'), { code: 'ENOENT' }));
  expect(await resolveDirectory({ path: '~/new' })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
});
it('validates the nearest existing ancestor before recursive mkdir and validates its result', async () => {
  vi.mocked(validateDirectory).mockRejectedValueOnce(Object.assign(new Error('missing'), { code: 'ENOENT' })).mockResolvedValueOnce('parent').mockResolvedValueOnce('created');
  expect(await createDirectory({ path: '~/new/a' })).toEqual({ ok: true, value: { cwd: 'created' } });
  expect(io.mkdir).toHaveBeenCalledWith(expect.any(String), { recursive: true });
  expect(validateDirectory).toHaveBeenCalledTimes(3);
});
it.each(['\\\\server\\share\\new', '\\\\?\\C:\\new', '\0'])('rejects unsafe create requests before filesystem mutation: %s', async input => {
  expect(await createDirectory({ path: input })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(io.mkdir).not.toHaveBeenCalled();
});
it('does not disguise permission failures while checking links as an empty success', async () => {
  io.readdir.mockResolvedValueOnce([{ name: 'link', isDirectory: () => false, isSymbolicLink: () => true }]);
  io.stat.mockRejectedValueOnce(Object.assign(new Error('denied'), { code: 'EACCES' }));
  expect(await completeDirectory({ path: '' })).toMatchObject({ ok: false, error: { code: 'VALIDATION', message: expect.stringContaining('not accessible') } });
});
