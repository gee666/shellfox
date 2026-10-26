import { describe, expect, it } from 'vitest';
import { normalizeDirectoryCompletionPath, visibleDirectoryCompletions } from './directory-completion';

describe('directory completion comparison', () => {
  it.each([
    ['~/work', '~/work/'],
    ['~/work/', '~/work/'],
    ['"~/work"', '~/work/'],
    ["  '~/work/'  ", '~/work/'],
    ['./work space', './work space/'],
    ['~/work\\', '~/work\\/'],
    ['../work', '../work/'],
    ['/home/user/work', '/home/user/work/'],
    ['"C:\\work space"', 'C:\\work space\\'],
    ['C:/work', 'C:/work/'],
    ['~\\work', '~\\work\\'],
    ['work', 'work\\'],
    [String.raw`'\\server\share\work'`, String.raw`\\server\share\work` + '\\'],
    ['"//wsl.localhost/Ubuntu/home/user/work/"', '//wsl.localhost/Ubuntu/home/user/work/'],
  ])('hides a sole exact match for %j', (input, match) => {
    expect(normalizeDirectoryCompletionPath(input)).toBe(normalizeDirectoryCompletionPath(match));
    expect(visibleDirectoryCompletions(input, [match])).toEqual([]);
  });

  it.each(['~/work', '~/work/', '"~/work"'])('keeps exact and longer siblings for %j', input => {
    const matches = Object.freeze(['~/work/', '~/workshop/']);
    expect(visibleDirectoryCompletions(input, matches)).toBe(matches);
  });

  it.each([
    ['~/wor', '~/work/'],
    ['~/work', '~/work\\/'],
    ['~/Work', '~/work/'],
    [' work ', 'work/'],
    ['" work "', 'work/'],
    ['"~/work', '~/work/'],
    ['./work', 'work/'],
    ['work', '~/work/'],
    ['C:\\work', 'C:\\workshop\\'],
  ])('does not hide a different path for %j', (input, match) => {
    expect(visibleDirectoryCompletions(input, [match])).toEqual([match]);
  });

  it('does not alter internal separators, case or filename spaces', () => {
    expect(normalizeDirectoryCompletionPath('  "C:\\My Folder\\"  ')).toBe('C:\\My Folder');
    expect(normalizeDirectoryCompletionPath(' work space /')).toBe(' work space ');
    expect(visibleDirectoryCompletions('', [])).toEqual([]);
  });
});
