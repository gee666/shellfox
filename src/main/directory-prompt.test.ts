import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mkdir, mkdtemp, rm, symlink, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { DIRECTORY_COMPLETION_BYTES, DIRECTORY_COMPLETION_LIMIT, requestSchemas, responseSchemas } from '../shared/schemas';

const state = vi.hoisted(() => ({ home: '', validate: vi.fn() }));
vi.mock('node:os', async importOriginal => ({ ...await importOriginal<typeof import('node:os')>(), homedir: () => state.home }));
vi.mock('./directory', () => ({ validateDirectory: state.validate }));
import { completeDirectory, directoryCompletionTarget, directoryPromptPath, getHomeDirectory, resolveDirectory } from './directory-prompt';

beforeEach(async () => {
  await mkdir('tmp', { recursive: true });
  state.home = await mkdtemp(path.resolve('tmp/directory-prompt-'));
  state.validate.mockReset();
  state.validate.mockImplementation(async (cwd: string) => cwd);
});
afterEach(async () => { await rm(state.home, { recursive: true, force: true }); });

it('returns os.homedir, not the application working directory', () => {
  expect(getHomeDirectory()).toEqual({ ok: true, value: { cwd: state.home } });
  expect(state.home).not.toBe(process.cwd());
  expect(state.validate).not.toHaveBeenCalled();
});

it.each(['', '~', '.', 'work space', '~/work space', '"~/work space"', "'~/work space'", '  "~/work space"  ', '../sibling'])('resolves %j relative to home and passes the absolute path to the existing validator', async input => {
  const expanded = ['', '~', '.'].includes(input) ? state.home
    : input === '../sibling' ? path.resolve(state.home, '../sibling') : path.join(state.home, 'work space');
  state.validate.mockResolvedValueOnce(expanded);
  expect(await resolveDirectory({ path: input })).toEqual({ ok: true, value: { cwd: expanded } });
  expect(state.validate).toHaveBeenCalledExactlyOnceWith(expanded);
});

it('preserves spaces inside quotes and unquoted filenames, expands no variables or shell expressions', () => {
  expect(directoryPromptPath('" spaced "')).toBe(path.join(state.home, ' spaced '));
  expect(directoryPromptPath(' spaced ')).toBe(path.join(state.home, ' spaced '));
  expect(directoryPromptPath('"\'literal\'"')).toBe(path.join(state.home, "'literal'"));
  expect(directoryPromptPath('~//nested')).toBe(path.join(state.home, 'nested'));
  for (const input of ['$HOME', '%USERPROFILE%', '~someone', '$(touch sentinel)', 'a; echo b']) {
    expect(directoryPromptPath(input)).toBe(path.join(state.home, input));
  }
});

it('keeps native absolute paths and returns the validator result', async () => {
  const absolute = path.join(state.home, 'folder');
  state.validate.mockResolvedValueOnce(absolute);
  expect(await resolveDirectory({ path: '"' + absolute + '"' })).toEqual({ ok: true, value: { cwd: absolute } });
  expect(state.validate).toHaveBeenCalledWith(absolute);
});

it.each([
  [new Error('Not a directory'), 'VALIDATION'],
  [Object.assign(new Error('missing'), { code: 'ENOENT' }), 'NOT_FOUND'],
  [Object.assign(new Error('denied'), { code: 'EACCES' }), 'VALIDATION'],
])('returns an explicit failure when existing directory validation rejects: %s', async (error, code) => {
  state.validate.mockRejectedValueOnce(error);
  expect(await resolveDirectory({ path: 'folder' })).toMatchObject({ ok: false, error: { code, message: expect.stringContaining('directory') } });
});

it.each(['bad\0path', 'bad\npath', 'x'.repeat(32768)])('rejects invalid prompt text before filesystem validation', async input => {
  expect(await resolveDirectory({ path: input })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await completeDirectory({ path: input })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(state.validate).not.toHaveBeenCalled();
});

async function folders(...names: string[]): Promise<void> {
  await Promise.all(names.map(name => mkdir(path.join(state.home, name), { recursive: true })));
}

it('lists only directories in deterministic name order and includes spaces', async () => {
  await folders('zeta', 'alpha', 'my folder');
  await writeFile(path.join(state.home, 'file.txt'), 'not a directory');
  expect(await completeDirectory({ path: '' })).toEqual({ ok: true, value: { matches: ['alpha', 'my folder', 'zeta'].map(name => name + path.sep) } });
});

it('matches only the final component, preserves prefixes and descends after a separator', async () => {
  await folders('work space/alpha', 'work space/beta', 'other');
  for (const input of ['work space/a', './work space/a', '~/work space/a', '"~/work space/a"']) {
    const prefix = input.replaceAll('"', '').slice(0, -1);
    expect(await completeDirectory({ path: input })).toEqual({ ok: true, value: { matches: [prefix + 'alpha/'] } });
  }
  expect(await completeDirectory({ path: 'work' })).toEqual({ ok: true, value: { matches: ['work space' + path.sep] } });
  expect(await completeDirectory({ path: 'work space/' })).toEqual({ ok: true, value: { matches: ['work space/alpha/', 'work space/beta/'] } });
  expect(await completeDirectory({ path: '~' })).toEqual({ ok: true, value: { matches: ['~' + path.sep + 'other' + path.sep, '~' + path.sep + 'work space' + path.sep] } });
  const absolute = state.home + path.sep;
  expect(await completeDirectory({ path: absolute + 'oth' })).toEqual({ ok: true, value: { matches: [absolute + 'other' + path.sep] } });
});

it('uses case-insensitive matching on Windows and case-sensitive matching on POSIX', async () => {
  await folders('Alpha');
  expect(await completeDirectory({ path: 'al' })).toEqual({ ok: true, value: { matches: process.platform === 'win32' ? ['Alpha' + path.sep] : [] } });
});

it('includes links to directories', async () => {
  await folders('target');
  await symlink(path.join(state.home, 'target'), path.join(state.home, 'linked'), process.platform === 'win32' ? 'junction' : 'dir');
  expect(await completeDirectory({ path: 'link' })).toEqual({ ok: true, value: { matches: ['linked' + path.sep] } });
});

it.skipIf(process.platform === 'win32')('excludes links to files, dangling links and names with control characters', async () => {
  await writeFile(path.join(state.home, 'file'), 'file');
  await symlink(path.join(state.home, 'file'), path.join(state.home, 'file-link'));
  await symlink(path.join(state.home, 'missing'), path.join(state.home, 'broken'));
  await folders('bad\nname');
  expect(await completeDirectory({ path: '' })).toEqual({ ok: true, value: { matches: [] } });
});

it('distinguishes no matches from nonexistent or non-directory parents', async () => {
  await writeFile(path.join(state.home, 'file'), 'file');
  expect(await completeDirectory({ path: 'absent-prefix' })).toEqual({ ok: true, value: { matches: [] } });
  for (const input of ['missing/', 'file/']) {
    expect(await completeDirectory({ path: input })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND', message: expect.stringContaining('Cannot complete path') } });
  }
});

it('bounds the sorted response to 100 matches', async () => {
  const names = Array.from({ length: DIRECTORY_COMPLETION_LIMIT + 10 }, (_, i) => 'folder-' + String(i).padStart(3, '0'));
  await folders(...names.reverse());
  const result = await completeDirectory({ path: '' });
  expect(result).toEqual({ ok: true, value: { matches: names.sort().slice(0, DIRECTORY_COMPLETION_LIMIT).map(name => name + path.sep) } });
  expect(responseSchemas.completeDirectory.safeParse(result).success).toBe(true);
});

it('bounds total UTF-8 match text even with a long preserved prefix', async () => {
  await folders(...Array.from({ length: 100 }, (_, i) => 'folder-' + i));
  // Redundant ./ segments keep filesystem paths short while exercising the wire limit.
  const prefix = './'.repeat(1000);
  const result = await completeDirectory({ path: prefix });
  expect(result.ok).toBe(true);
  if (!result.ok) return;
  expect(result.value.matches.length).toBeGreaterThan(0);
  expect(result.value.matches.length).toBeLessThan(DIRECTORY_COMPLETION_LIMIT);
  expect(result.value.matches.reduce((sum, match) => sum + Buffer.byteLength(match), 0)).toBeLessThanOrEqual(DIRECTORY_COMPLETION_BYTES);
  expect(responseSchemas.completeDirectory.safeParse(result).success).toBe(true);
});

describe('Windows syntax, tested independently of the host OS', () => {
  const home = 'C:\\Users\\Tester';
  it.each([
    ['work space/sub', 'C:\\Users\\Tester\\work space\\sub'],
    ['work space\\sub', 'C:\\Users\\Tester\\work space\\sub'],
    ['~\\work space', 'C:\\Users\\Tester\\work space'],
    ['~\\\\work space', 'C:\\Users\\Tester\\work space'],
    ['D:/work space', 'D:\\work space'],
    ['\\rooted', 'C:\\rooted'],
    ['\\\\server\\share\\work space', '\\\\server\\share\\work space'],
    ['//wsl.localhost/Ubuntu/home/tester', '\\\\wsl.localhost\\Ubuntu\\home\\tester'],
  ])('expands %j without losing drive or UNC roots', (input, expected) => {
    expect(directoryPromptPath(input, home, 'win32')).toBe(expected);
  });
  it.each(['C:', 'C:relative', '\\\\server', '\\\\?\\C:\\folder', '\\\\.\\pipe\\name'])('rejects ambiguous or device path %j', input => {
    expect(() => directoryPromptPath(input, home, 'win32')).toThrow();
  });
  it.each([
    ['~\\work\\a', 'C:\\Users\\Tester\\work', '~\\work\\', 'a', '\\'],
    ['./work/a', 'C:\\Users\\Tester\\work', './work/', 'a', '/'],
    ['D:/work\\a', 'D:\\work', 'D:/work\\', 'a', '\\'],
    ['\\\\server\\share', '\\\\server\\share\\', '\\\\server\\share\\', '', '\\'],
    ['//wsl$/Ubuntu/home/a', '\\\\wsl$\\Ubuntu\\home', '//wsl$/Ubuntu/home/', 'a', '/'],
  ])('preserves separators and prefix for %j', (input, directory, prefix, fragment, separator) => {
    expect(directoryCompletionTarget(input, home, 'win32')).toEqual({ directory, prefix, fragment, separator });
  });
});

it('does not reinterpret Windows absolute paths as POSIX relative paths', () => {
  for (const input of ['C:\\folder', '\\\\server\\share', '//server/share']) expect(() => directoryPromptPath(input, '/home/tester', 'linux')).toThrow('require Windows');
});

it('validates strict requests and bounded completion responses', () => {
  for (const method of ['getHomeDirectory', 'resolveDirectory', 'completeDirectory'] as const) {
    const valid = method === 'getHomeDirectory' ? {} : { path: '' };
    expect(requestSchemas[method].safeParse(valid).success).toBe(true);
    expect(requestSchemas[method].safeParse({ ...valid, command: 'echo bad' }).success).toBe(false);
  }
  expect(requestSchemas.resolveDirectory.safeParse({}).success).toBe(false);
  expect(requestSchemas.completeDirectory.safeParse({ path: 42 }).success).toBe(false);
  expect(responseSchemas.completeDirectory.safeParse({ ok: true, value: { matches: Array(101).fill('dir/') } }).success).toBe(false);
  expect(responseSchemas.completeDirectory.safeParse({ ok: true, value: { matches: Array(3).fill('雪'.repeat(10000)) } }).success).toBe(false);
});
