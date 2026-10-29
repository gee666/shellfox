import type { IBufferLine, IBufferRange, ILink, ILinkProvider, Terminal } from '@xterm/xterm';

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

/** Box-drawing column separators used by TUI tables (pi, Claude Code, rich, etc.). */
const BORDERS = new Set([...'│┃║┆┇┊┋╎╏']);
/** Trailing blank cells allowed between a broken URL and its column edge (cell padding). */
const EDGE_SLACK = 2;
/** Extra indentation allowed for a continuation row (list hanging indents). */
const INDENT_SLACK = 4;
/** How far back/forward a URL broken across rows of a table column is followed. */
const MAX_CHAIN_ROWS = 24;

interface Region { y: number; left: number; right: number; text: string; xs: number[] }
interface Segment { y: number; start: number; end: number }
type LinkHooks = { hover?(url: string): void; leave?(): void };

/** Row text split at table borders. Each code unit keeps its 0-based cell x; wide-char spacers are skipped. */
function rowRegions(line: IBufferLine, y: number): Region[] {
  const regions: Region[] = [];
  let current: Region = { y, left: 0, right: line.length, text: '', xs: [] };
  for (let x = 0; x < line.length; x++) {
    const cell = line.getCell(x);
    if (!cell || cell.getWidth() === 0) continue;
    const chars = cell.getChars() || ' ';
    if (BORDERS.has(chars)) {
      current.right = x; regions.push(current);
      current = { y, left: x + 1, right: line.length, text: '', xs: [] };
      continue;
    }
    current.text += chars;
    for (let i = 0; i < chars.length; i++) current.xs.push(x);
  }
  regions.push(current);
  return regions;
}

/** Blank-padded distance from code unit `index` to the region edge, or -1 if non-blank text follows. */
function edgeGap(region: Region, index: number): number {
  const rest = region.text.slice(index);
  if (rest.trim()) return -1;
  return region.right - (index < region.xs.length ? region.xs[index] : region.right);
}

/**
 * URLs a TUI hard-broke inside a table column or indented paragraph: a URL that runs to the
 * column edge continues on the next row in the same column, starting at the column's text margin.
 * Each row piece is returned separately because xterm ranges are linear and would span other columns.
 */
function columnLinks(terminal: Terminal, row: number): { url: string; segments: Segment[] }[] {
  const buffer = terminal.buffer.active;
  const cache = new Map<number, Region[] | null>();
  const regionsAt = (y: number) => {
    if (!cache.has(y)) {
      const line = buffer.getLine(y);
      // Soft-wrapped rows belong to the plain provider.
      cache.set(y, line && !line.isWrapped && !buffer.getLine(y + 1)?.isWrapped ? rowRegions(line, y) : null);
    }
    return cache.get(y)!;
  };
  const found: { url: string; segments: Segment[] }[] = [];
  for (let y = Math.max(0, row - 1 - MAX_CHAIN_ROWS); y <= row - 1; y++) {
    for (const region of regionsAt(y) ?? []) {
      const match = [...region.text.matchAll(/https?:\/\/[^\s<>"'`]+/g)].at(-1);
      if (!match) continue;
      const startIndex = match.index, endIndex = startIndex + match[0].length;
      if (edgeGap(region, endIndex) < 0 || edgeGap(region, endIndex) > EDGE_SLACK) continue;
      const margin = region.xs[region.text.search(/\S/)]!;
      const pieces: { region: Region; from: number; to: number }[] = [{ region, from: startIndex, to: endIndex }];
      let joined = match[0];
      for (let next = y + 1; next <= y + MAX_CHAIN_ROWS && joined.length <= MAX_LINK_LENGTH; next++) {
        const below = regionsAt(next)?.find(r => r.left === region.left && r.right === region.right);
        if (!below) break;
        const from = below.text.search(/\S/);
        if (from < 0 || below.xs[from]! < margin || below.xs[from]! > margin + INDENT_SLACK) break;
        const run = /^[^\s<>"'`]+/.exec(below.text.slice(from))![0];
        if (/^https?:\/\//.test(run)) break;
        // Prose wrapped under a complete URL ("next row") is not a continuation; a URL tail
        // either ends its row piece or still looks like a URL.
        if (below.text.slice(from + run.length).trim() && !/[/?#=&%~+]/.test(run)) break;
        pieces.push({ region: below, from, to: from + run.length });
        joined += run;
        const gap = edgeGap(below, from + run.length);
        if (gap < 0 || gap > EDGE_SLACK) break;
      }
      if (pieces.length < 2) continue;
      const link = findWebLinks(joined)[0];
      if (!link || link.start !== 0) continue;
      // Map the trimmed URL back onto row pieces.
      let remaining = link.end;
      const segments: Segment[] = [];
      for (const piece of pieces) {
        if (remaining <= 0) break;
        const length = Math.min(piece.to - piece.from, remaining);
        remaining -= length;
        segments.push({ y: piece.region.y + 1, start: piece.region.xs[piece.from]! + 1, end: piece.region.xs[piece.from + length - 1]! + 1 });
      }
      if (segments.length >= 2) found.push({ url: link.url, segments });
    }
  }
  return found.filter(link => link.segments.some(segment => segment.y === row));
}

/** Plain-text URLs, including URLs soft-wrapped across rows and URLs broken inside table columns. */
export function webLinkProvider(terminal: Terminal, open: (url: string, event: MouseEvent) => void, hooks: LinkHooks = {}): ILinkProvider {
  const decorate = (url: string, range: IBufferRange): ILink => ({
    text: url, range, decorations: { underline: true, pointerCursor: true },
    activate: (event: MouseEvent) => { if (event.button === 0) open(url, event); },
    hover: () => hooks.hover?.(url), leave: () => hooks.leave?.(),
  });
  return {
    provideLinks(row, callback) {
      const columns = columnLinks(terminal, row).flatMap(link => link.segments
        .filter(segment => segment.y === row)
        .map(segment => decorate(link.url, { start: { x: segment.start, y: row }, end: { x: segment.end, y: row } })));
      const overlaps = (range: IBufferRange) => columns.some(column =>
        range.start.y <= row && range.end.y >= row
        && (range.start.y < row ? 1 : range.start.x) <= column.range.end.x
        && (range.end.y > row ? Infinity : range.end.x) >= column.range.start.x);
      const links = [...plainLinks(terminal, row).map(link => decorate(link.url, link.range)).filter(link => !overlaps(link.range)), ...columns];
      callback(links.length ? links : undefined);
    },
  };
}

/** URLs in the soft-wrapped logical line containing `row`. */
function plainLinks(terminal: Terminal, row: number): { url: string; range: IBufferRange }[] {
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
  return findWebLinks(text)
    .map(link => ({ url: link.url, start: cells[link.start], end: cells[link.end - 1] }))
    .filter(({ start, end }) => start && end && start.y <= row && end.y >= row)
    .map(({ url, start, end }) => ({ url, range: { start: start!, end: end! } }));
}
