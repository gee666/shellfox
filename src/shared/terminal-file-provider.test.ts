// @vitest-environment jsdom
import { Terminal } from '@xterm/headless';
import type { ILink } from '@xterm/xterm';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { fileLinkProvider } from './terminal-file-provider';
import { parseTerminalFileTarget } from './terminal-file-links';

const terminals: Terminal[] = [];
afterEach(() => { for (const terminal of terminals.splice(0)) terminal.dispose(); });
async function screen(rows: string[], cols = Math.max(...rows.map(row => row.length))) {
  const terminal = new Terminal({ cols, rows: 12, allowProposedApi: true });
  terminals.push(terminal);
  await new Promise<void>(resolve => terminal.write(rows.join('\r\n'), resolve));
  return terminal;
}
function linksAt(terminal: Terminal, row: number) {
  const open = vi.fn(), hover = vi.fn(), leave = vi.fn();
  const callback = vi.fn<(links: ILink[] | undefined) => void>();
  fileLinkProvider(terminal, open, { hover, leave }).provideLinks(row, callback);
  return { links: callback.mock.calls[0]![0] ?? [], open, hover, leave };
}
function table(rows: string[], width = Math.max(...rows.map(row => row.length))) {
  return rows.map(text => `│ ${text.padEnd(width)} │ note │`);
}

describe('terminal file provider', () => {
  it.each([
    '/home/me/project/src/main.ts:12:3',
    'C:\\work\\project\\src\\main.ts:12:3',
    './src/very-long-folder/main.ts:12',
    '../some/very-long-folder/',
    '~/some/very-long-folder',
    'file:///home/me/project/main.ts#L12C3',
    '"C:\\my project\\src\\main.ts:12:3"',
  ])('maps soft-wrapped %s from every row to the complete target', async text => {
    const cols = 16, prefix = 'see ', target = text.replace(/^"|"$/g, '');
    const terminal = await screen([prefix + text], cols);
    const start = prefix.length + (text.startsWith('"') ? 1 : 0);
    const end = start + target.length - 1;
    for (let row = 1; row <= Math.floor(end / cols) + 1; row++) {
      const { links, open, hover, leave } = linksAt(terminal, row);
      expect(links).toHaveLength(1);
      const link = links[0]!;
      expect(link.text).toBe(target);
      expect(link.range).toEqual({
        start: { x: start % cols + 1, y: Math.floor(start / cols) + 1 },
        end: { x: end % cols + 1, y: Math.floor(end / cols) + 1 },
      });
      link.activate(new MouseEvent('click', { button: 2 }), link.text);
      expect(open).not.toHaveBeenCalled();
      const event = new MouseEvent('click', { button: 0 });
      link.activate(event, link.text);
      expect(open).toHaveBeenCalledWith(expect.objectContaining(parseTerminalFileTarget(target)!), event);
      link.hover?.(event, link.text); link.leave?.(event, link.text);
      expect(hover).toHaveBeenCalledWith(target); expect(leave).toHaveBeenCalledOnce();
    }
    expect(linksAt(terminal, Math.floor(end / cols) + 2).links).toEqual([]);
  });

  it('recognizes a hard-broken path following prose with an apostrophe', async () => {
    const terminal = await screen(["  here's ./src/long-", '  folder/main.ts'], 20);
    for (const row of [1, 2]) {
      expect(linksAt(terminal, row).links.map(link => link.text)).toEqual(['./src/long-folder/main.ts']);
    }
  });

  it('joins a table path and split line/column suffix without linking the adjacent column', async () => {
    const pieces = ['./src/very-long-', 'folder/main.ts:1', '2:3, then'];
    const terminal = await screen(table(pieces));
    for (const [index, length] of [pieces[0]!.length, pieces[1]!.length, 3].entries()) {
      const row = index + 1, { links, open } = linksAt(terminal, row);
      expect(links).toHaveLength(1);
      const link = links[0]!;
      expect(link.text).toBe('./src/very-long-folder/main.ts:12:3');
      expect(link.range).toEqual({ start: { x: 3, y: row }, end: { x: length + 2, y: row } });
      const event = new MouseEvent('click');
      link.activate(event, link.text);
      expect(open).toHaveBeenCalledWith(expect.objectContaining({ path: './src/very-long-folder/main.ts', line: 12, column: 3 }), event);
    }
  });

  it.each([
    { pieces: ['/home/me/long-', 'folder/subdir/'], target: '/home/me/long-folder/subdir/' },
    { pieces: ['C:\\work\\long-', 'folder\\subdir'], target: 'C:\\work\\long-folder\\subdir' },
    { pieces: ['relative/long-', 'folder/'], target: 'relative/long-folder/' },
    { pieces: ['"C:\\my project\\', 'some folder"'], target: 'C:\\my project\\some folder' },
    { pieces: ['file:///work/a.ts#L1', '2C3'], target: 'file:///work/a.ts#L12C3' },
  ])('joins hard-broken folders and quoted/URI paths: $target', async ({ pieces, target }) => {
    const terminal = await screen(table(pieces));
    for (let row = 1; row <= pieces.length; row++) {
      const { links, open } = linksAt(terminal, row);
      expect(links.map(link => link.text)).toEqual([target]);
      const event = new MouseEvent('click'), link = links[0]!;
      link.activate(event, link.text);
      expect(open).toHaveBeenCalledWith(expect.objectContaining(parseTerminalFileTarget(target)!), event);
    }
  });

  it('joins indented path paragraphs while keeping prose and independent targets separate', async () => {
    const terminal = await screen([
      '  see ./src/very-long-',
      '  folder/a.ts:12:3, ok',
      '  ./other/complete.ts',
      '  next row',
      '  /other/new.ts',
      '  ./independent.ts',
      '        far/away.ts',
    ], 23);
    expect(linksAt(terminal, 1).links.map(link => link.text)).toEqual(['./src/very-long-folder/a.ts:12:3']);
    expect(linksAt(terminal, 2).links.map(link => link.text)).toEqual(['./src/very-long-folder/a.ts:12:3']);
    expect(linksAt(terminal, 3).links.map(link => link.text)).toEqual(['./other/complete.ts']);
    expect(linksAt(terminal, 4).links).toEqual([]);
    expect(linksAt(terminal, 5).links.map(link => link.text)).toEqual(['/other/new.ts']);
    expect(linksAt(terminal, 6).links.map(link => link.text)).toEqual(['./independent.ts']);
    expect(linksAt(terminal, 7).links.map(link => link.text)).toEqual(['far/away.ts']);
  });

  it('does not join across table borders, column changes, blank rows or normal newlines', async () => {
    for (const rows of [
      ['./src/long-', '────────────', 'folder/main.ts'],
      ['./src/long-', '', 'folder/main.ts'],
      ['./src/long-', '│ folder/main.ts │'],
      ['./src/long- more', 'folder/main.ts'],
    ]) {
      const terminal = await screen(rows);
      expect(linksAt(terminal, 1).links.map(link => link.text)).toEqual(['./src/long-']);
      expect(linksAt(terminal, rows.length).links.map(link => link.text)).toEqual(['folder/main.ts']);
    }
  });

  it('keeps schemes and invalid joined source positions out of file targets', async () => {
    const terminal = await screen(table(['https://example/', 'main.ts']));
    expect(linksAt(terminal, 1).links).toEqual([]);
    const invalid = await screen(table(['./src/main.ts:', '0:3']));
    expect(linksAt(invalid, 2).links).toEqual([]);
  });

  it.each([
    { text: './abcdefgh/界/name.ts', end: 10 },
    { text: './abcdefgh/界', end: 2 },
  ])('skips the empty cell before a wide character wraps: $text', async ({ text, end }) => {
    const terminal = await screen([text], 12);
    for (const row of [1, 2]) {
      const [link] = linksAt(terminal, row).links;
      expect(link?.text).toBe(text);
      expect(link?.range).toEqual({ start: { x: 1, y: 1 }, end: { x: end, y: 2 } });
    }
  });

  it('includes both cells of a wide character at a table piece edge', async () => {
    const terminal = await screen(['│ ./abc/界 │ note │', '│ folder/  │ note │'], 24);
    const [link] = linksAt(terminal, 1).links;
    expect(link?.text).toBe('./abc/界folder/');
    expect(link?.range).toEqual({ start: { x: 3, y: 1 }, end: { x: 10, y: 1 } });
  });

  it('maps Unicode code units to terminal cells before and within wrapped paths', async () => {
    const terminal = await screen(['界 ./src/日本語/😀e\u0301.ts:12:3'], 16);
    for (const row of [1, 2]) {
      const [link] = linksAt(terminal, row).links;
      expect(link?.text).toBe('./src/日本語/😀e\u0301.ts:12:3');
      expect(link?.range).toEqual({ start: { x: 4, y: 1 }, end: { x: 10, y: 2 } });
    }
  });
});
