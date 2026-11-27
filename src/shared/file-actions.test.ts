import { describe, expect, it, vi } from 'vitest';
import { failure, success } from './contracts';
import type { FileApi } from './file-contracts';
import { fileNameError, joinFilePath, openFileTarget } from './file-actions';

const folderList = (path: string) => ({ path, parentPath: null, entries: [], truncated: false });
const listing = (path: string) => success(folderList(path));
function api(read: FileApi['read'], list: FileApi['list']) {
  return { read: vi.fn(read), list: vi.fn(list) };
}

describe('file actions', () => {
  it('joins relative paths in the host separator style', () => {
    expect(joinFilePath('/work/project', 'tmp/../REPORT.md')).toBe('/work/project/REPORT.md');
    expect(joinFilePath('/', './a/b')).toBe('/a/b');
    expect(joinFilePath('/work', '../../..')).toBe('/');
    expect(joinFilePath('C:\\work', '..\\x\\y.txt')).toBe('C:\\x\\y.txt');
    expect(joinFilePath('C:\\', '..')).toBe('C:\\');
  });

  it('opens files, falls back to folders and reports the searched absolute path', async () => {
    const file = api(async input => success({ kind: 'text', path: '/w/' + input.path, text: '', size: 0, revision: 'a'.repeat(64), mimeType: 'text/plain' }), async () => failure('NOT_FOUND', 'x'));
    expect(await openFileTarget(file, { path: 'a.ts' })).toMatchObject({ kind: 'file', file: { path: '/w/a.ts' } });
    const folder = api(async () => failure('UNSUPPORTED', 'Only regular files can be transferred.'), async input => listing('/w/' + input.path));
    expect(await openFileTarget(folder, { path: 'out/' })).toEqual({ kind: 'directory', listing: folderList('/w/out/') });
    const missing = api(async () => failure('NOT_FOUND', 'File or directory not found.'), async input => input.path === '.' ? listing('/cwd') : input.path === '~' ? listing('/home/me') : failure('NOT_FOUND', 'x'));
    expect(await openFileTarget(missing, { sessionId: 's', path: 'REPORT.md' })).toEqual({ kind: 'error', path: '/cwd/REPORT.md', error: expect.objectContaining({ message: 'File or directory not found.' }) });
    expect(await openFileTarget(missing, { path: '~/notes/a.md' })).toMatchObject({ path: '/home/me/notes/a.md' });
    expect(await openFileTarget(missing, { path: '/abs/a.md' })).toMatchObject({ path: '/abs/a.md' });
    expect(missing.list).not.toHaveBeenCalledWith({ path: '.' });
  });

  it('keeps the requested path when the working directory is unknown', async () => {
    const unknown = api(async () => failure('NOT_FOUND', 'Missing'), async () => failure('UNSUPPORTED', 'No cwd'));
    expect(await openFileTarget(unknown, { path: 'REPORT.md' })).toMatchObject({ kind: 'error', path: 'REPORT.md' });
  });

  it('accepts any extension but only a single portable file name', () => {
    expect(fileNameError('report.anything')).toBeNull();
    expect(fileNameError('Makefile')).toBeNull();
    for (const name of ['', ' ', '.', '..', 'a/b', 'a\\b', 'a:b', 'trailing.']) expect(fileNameError(name)).not.toBeNull();
  });
});
