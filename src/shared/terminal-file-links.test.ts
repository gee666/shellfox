import { describe, expect, it } from 'vitest';
import { findTerminalFileLinks, parseTerminalFileTarget } from './terminal-file-links';

describe('terminal file targets', () => {
  it('parses local POSIX, Windows, relative and file URI targets with source positions', () => {
    expect(parseTerminalFileTarget('/home/me/app.py:12:4')).toEqual({ path: '/home/me/app.py', line: 12, column: 4 });
    expect(parseTerminalFileTarget('C:\\work\\app.js:9')).toEqual({ path: 'C:\\work\\app.js', line: 9 });
    expect(parseTerminalFileTarget('file:///C:/my%20project/app.xml')).toEqual({ path: 'C:/my project/app.xml' });
    expect(parseTerminalFileTarget('file:///home/me/app.md')).toEqual({ path: '/home/me/app.md' });
    expect(parseTerminalFileTarget('../LICENSE')).toEqual({ path: '../LICENSE' });
    expect(parseTerminalFileTarget('README.md')).toEqual({ path: 'README.md' });
    expect(parseTerminalFileTarget('file:///C:/work/app.ts#L14C3')).toEqual({ path: 'C:/work/app.ts', line: 14, column: 3 });
    expect(parseTerminalFileTarget('file:///work/app.py#L12-L20')).toEqual({ path: '/work/app.py', line: 12 });
    expect(parseTerminalFileTarget('file:///work/app.py%3A12%3A4')).toEqual({ path: '/work/app.py', line: 12, column: 4 });
  });
  it('accepts folders without extensions and relative folders with trailing separators', () => {
    for (const path of ['/home/me/project/', 'C:\\work\\folder', './src/shared', '../folder/', '~/project', 'src/shared/', 'src\\shared\\']) {
      expect(parseTerminalFileTarget(path)).toEqual({ path });
    }
  });
  it('keeps paths after apostrophes and unmatched quotes in prose', () => {
    for (const text of ["Here's ./src/main.ts", 'an unmatched "quote before ./src/main.ts']) {
      expect(findTerminalFileLinks(text).map(link => link.text)).toEqual(['./src/main.ts']);
    }
  });
  it('stops file tokens at table borders without requiring padding', () => {
    expect(findTerminalFileLinks('│./src/main.ts:12:3│src/shared/│').map(link => link.text)).toEqual(['./src/main.ts:12:3', 'src/shared/']);
  });
  it('does not treat network URLs, arbitrary protocols, control characters or invalid positions as files', () => {
    for (const value of ['https://example.com/a.py', 'javascript:alert(1)', 'file://other-host/etc/passwd', 'data:text/html,x', 'app.py:0', 'a.py:9007199254740992', '/etc/\npasswd', 'file:///a%00b', 'file:///x?command=y', 'file:///x#javascript:alert(1)', 'file:///x#L0', 'plain']) {
      expect(parseTerminalFileTarget(value), value).toBeNull();
    }
  });
  it('matches quoted spaces and multiple paths without swallowing prose or web links', () => {
    const text = 'See "C:\\my project\\a.json" then ./src/main.ts:8:2, and (README.md). https://example.com/a.py';
    const links = findTerminalFileLinks(text);
    expect(links.map(({ path, line, column }) => ({ path, line, column }))).toEqual([
      { path: 'C:\\my project\\a.json', line: undefined, column: undefined },
      { path: './src/main.ts', line: 8, column: 2 },
      { path: 'README.md', line: undefined, column: undefined },
    ]);
    for (const link of links) expect(text.slice(link.start, link.end)).toBe(link.text);
  });
});
