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

  const screen = (rows: string[], wrapped: boolean[] = []) => {
    const width = Math.max(...rows.map(row => row.length));
    const line = (y: number) => y >= 0 && y < rows.length ? {
      isWrapped: !!wrapped[y], length: width,
      getCell: (x: number) => ({ getWidth: () => 1, getChars: () => (rows[y][x] ?? '').trim() }),
    } : undefined;
    return { buffer: { active: { getLine: line } } } as unknown as Terminal;
  };
  const linksAt = (terminal: Terminal, row: number, hooks?: Parameters<typeof webLinkProvider>[2]) => {
    const callback = vi.fn(), open = vi.fn();
    webLinkProvider(terminal, open, hooks).provideLinks(row, callback);
    return { links: (callback.mock.calls[0][0] ?? []) as any[], open };
  };
  it('joins URLs a TUI broke inside a table column, one range per row piece', () => {
    const table = screen([
      '┌──────────────────┬───────┐',
      '│ ramcore #603     │ None  │',
      '│ (https://a.io/st │ text  │',
      '│ ash/x/pr/603)    │ more  │',
      '├──────────────────┼───────┤',
      '│ https://b.io/one │ ok    │',
      '│ next row         │       │',
      '└──────────────────┴───────┘',
    ]);
    for (const [row, start, end] of [[3, 4, 18], [4, 3, 14]]) {
      const { links, open } = linksAt(table, row);
      expect(links).toHaveLength(1);
      expect(links[0].text).toBe('https://a.io/stash/x/pr/603');
      expect(links[0].range).toEqual({ start: { x: start, y: row }, end: { x: end, y: row } });
      links[0].activate({ button: 0 });
      expect(open).toHaveBeenCalledWith('https://a.io/stash/x/pr/603');
    }
    expect(linksAt(table, 2).links).toEqual([]);
    // A complete URL at the edge is not glued to unrelated text below that is not a URL continuation.
    expect(linksAt(table, 6).links.map(link => link.text)).toEqual(['https://b.io/one']);
    expect(linksAt(table, 7).links).toEqual([]);
  });
  it('joins hard-broken URLs in indented paragraphs and keeps neighbouring text and new URLs separate', () => {
    const page = screen([
      '  see https://x.io/aaaa',
      '  bbbb/cccc. Then',
      '  https://y.io/zzzzzz',
      '  https://z.io/k',
      '        far/away',
    ]);
    expect(linksAt(page, 1).links.map(link => link.text)).toEqual(['https://x.io/aaaabbbb/cccc']);
    expect(linksAt(page, 2).links.map(link => [link.text, link.range])).toEqual([['https://x.io/aaaabbbb/cccc', { start: { x: 3, y: 2 }, end: { x: 11, y: 2 } }]]);
    expect(linksAt(page, 3).links.map(link => link.text)).toEqual(['https://y.io/zzzzzz']);
    expect(linksAt(page, 4).links.map(link => link.text)).toEqual(['https://z.io/k']);
    expect(linksAt(page, 5).links).toEqual([]);
  });
  it('reports hovered URLs so hidden link targets can be shown', () => {
    const hover = vi.fn(), leave = vi.fn();
    const [link] = linksAt(screen(['https://example.com']), 1, { hover, leave }).links;
    link.hover(); link.leave();
    expect(hover).toHaveBeenCalledWith('https://example.com'); expect(leave).toHaveBeenCalled();
  });
});

