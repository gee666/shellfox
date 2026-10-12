import type { ILink, ILinkProvider, Terminal } from '@xterm/xterm';

export const MAX_LINK_LENGTH = 2048;
/** Only web links leave the terminal; file:, javascript: and custom schemes never do. */
export function isWebLink(value: string): boolean {
  if (value.length > MAX_LINK_LENGTH) return false;
  try { const url = new URL(value); return (url.protocol === 'http:' || url.protocol === 'https:') && !!url.hostname; }
  catch { return false; }
}

/** http(s) URLs in plain text as [start, end) code-unit ranges, without trailing punctuation. */
export function findWebLinks(text: string): { start: number; end: number; url: string }[] {
  const links: { start: number; end: number; url: string }[] = [];
  for (const match of text.matchAll(/https?:\/\/[^\s<>"'`]+/g)) {
    let url = match[0];
    for (;;) {
      const last = url.at(-1)!;
      if (/[.,;:!?*]/.test(last)) url = url.slice(0, -1);
      else if (last === ')' && (url.match(/\(/g)?.length ?? 0) < (url.match(/\)/g)?.length ?? 0)) url = url.slice(0, -1);
      else if (last === ']' && (url.match(/\[/g)?.length ?? 0) < (url.match(/\]/g)?.length ?? 0)) url = url.slice(0, -1);
      else break;
    }
    if (isWebLink(url)) links.push({ start: match.index, end: match.index + url.length, url });
  }
  return links;
}

/** Link provider for plain-text URLs, including URLs soft-wrapped across rows. */
export function webLinkProvider(terminal: Terminal, open: (url: string) => void): ILinkProvider {
  return {
    provideLinks(row, callback) {
      const buffer = terminal.buffer.active;
      let first = row - 1;
      while (first > 0 && buffer.getLine(first)?.isWrapped) first--;
      let last = row - 1;
      while (buffer.getLine(last + 1)?.isWrapped) last++;
      // Cell-accurate text: one position per UTF-16 code unit, skipping wide-char spacers.
      let text = '';
      const cells: { x: number; y: number }[] = [];
      for (let y = first; y <= last; y++) {
        const line = buffer.getLine(y);
        if (!line) break;
        for (let x = 0; x < line.length; x++) {
          const cell = line.getCell(x);
          if (!cell || cell.getWidth() === 0) continue;
          const chars = cell.getChars() || ' ';
          text += chars;
          for (let i = 0; i < chars.length; i++) cells.push({ x: x + 1, y: y + 1 });
        }
      }
      const links: ILink[] = findWebLinks(text)
        .map(link => ({ link, start: cells[link.start], end: cells[link.end - 1] }))
        .filter(({ start, end }) => start && end && start.y <= row && end.y >= row)
        .map(({ link, start, end }) => ({
          text: link.url, range: { start, end }, decorations: { underline: true, pointerCursor: true },
          activate: (event: MouseEvent) => { if (event.button === 0) open(link.url); },
        }));
      callback(links.length ? links : undefined);
    },
  };
}
