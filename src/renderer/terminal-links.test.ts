import { describe, expect, it, vi } from 'vitest';
import type { Terminal } from '@xterm/xterm';
import { findWebLinks, isWebLink, webLinkProvider } from './terminal-links';

describe('web links', () => {
  it('accepts only http(s) URLs with a host', () => {
    expect(isWebLink('https://example.com')).toBe(true);
    expect(isWebLink('http://localhost:3000/x')).toBe(true);
    for (const bad of ['javascript:alert(1)', 'file:///etc/passwd', 'http://', 'ftp://x.y', 'https://' + 'a'.repeat(2048)]) expect(isWebLink(bad)).toBe(false);
  });
  it('finds URLs and trims trailing punctuation but keeps balanced parentheses', () => {
    expect(findWebLinks('see https://example.com/a?b=1, then (http://x.io/y).').map(link => link.url)).toEqual(['https://example.com/a?b=1', 'http://x.io/y']);
    expect(findWebLinks('https://en.wikipedia.org/wiki/Foo_(bar)')[0].url).toBe('https://en.wikipedia.org/wiki/Foo_(bar)');
    expect(findWebLinks('"https://a.b/c" <https://d.e>').map(link => [link.start, link.end])).toEqual([[1, 14], [17, 28]]);
    expect(findWebLinks('no links, javascript:alert(1)')).toEqual([]);
  });
  it('provides cell ranges across soft-wrapped rows and opens on left click', () => {
    const rows = ['go https://exa', 'mple.com/x ok'], wrapped = [false, true];
    const line = (y: number) => y < rows.length ? {
      isWrapped: wrapped[y], length: 14,
      getCell: (x: number) => ({ getWidth: () => 1, getChars: () => rows[y][x] ?? '' }),
    } : undefined;
    const terminal = { buffer: { active: { getLine: line } } } as unknown as Terminal;
    const open = vi.fn(), provider = webLinkProvider(terminal, open);
    for (const row of [1, 2]) {
      const callback = vi.fn(); provider.provideLinks(row, callback);
      const [link] = callback.mock.calls[0][0];
      expect(link.text).toBe('https://example.com/x');
      expect(link.range).toEqual({ start: { x: 4, y: 1 }, end: { x: 10, y: 2 } });
      link.activate({ button: 2 }); link.activate({ button: 0 });
    }
    expect(open).toHaveBeenCalledTimes(2);
    const none = vi.fn(), plain = { isWrapped: false, length: 1, getCell: () => ({ getWidth: () => 1, getChars: () => 'x' }) };
    webLinkProvider({ buffer: { active: { getLine: (y: number) => y === 0 ? plain : undefined } } } as unknown as Terminal, open).provideLinks(1, none);
    expect(none).toHaveBeenCalledWith(undefined);
  });
});
