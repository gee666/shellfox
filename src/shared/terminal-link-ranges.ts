import type { IBufferLine, IBufferRange, Terminal } from '@xterm/xterm';

interface TextRange { start: number; end: number }
interface Region { y: number; left: number; right: number; text: string; xs: number[]; ends: number[] }
interface LinkMatcher<T extends TextRange> {
  find(text: string): T[];
  starts(text: string): TextRange[];
  continuation(text: string, joined: string): string | undefined;
  maxLength: number;
}
interface RangedLink<T> { link: T; range: IBufferRange }

/** Box-drawing column separators used by TUI tables. */
const BORDERS = new Set([...'│┃║┆┇┊┋╎╏']);
const EDGE_SLACK = 2;
const INDENT_SLACK = 4;
const MAX_CHAIN_ROWS = 24;

/** Each UTF-16 code unit keeps its cell x; wide-character spacers are skipped. */
function rowRegions(line: IBufferLine, y: number): Region[] {
  const regions: Region[] = [];
  let current: Region = { y, left: 0, right: line.length, text: '', xs: [], ends: [] };
  for (let x = 0; x < line.length; x++) {
    const cell = line.getCell(x);
    if (!cell || cell.getWidth() === 0) continue;
    const chars = cell.getChars() || ' ';
    if (BORDERS.has(chars)) {
      current.right = x; regions.push(current);
      current = { y, left: x + 1, right: line.length, text: '', xs: [], ends: [] };
      continue;
    }
    current.text += chars;
    for (let i = 0; i < chars.length; i++) {
      current.xs.push(x); current.ends.push(x + cell.getWidth());
    }
  }
  regions.push(current);
  return regions;
}

/** Blank-padded distance to the column edge, or -1 if non-blank text follows. */
function edgeGap(region: Region, index: number): number {
  if (region.text.slice(index).trim()) return -1;
  return region.right - (index < region.xs.length ? region.xs[index] : region.right);
}

/** Hard-broken links get one range per row, so they never span neighbouring table columns. */
function columnLinks<T extends TextRange>(terminal: Pick<Terminal, 'buffer'>, row: number, matcher: LinkMatcher<T>): RangedLink<T>[] {
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
  const found: RangedLink<T>[] = [];
  const claimed: IBufferRange[] = [];
  for (let y = Math.max(0, row - 1 - MAX_CHAIN_ROWS); y <= row - 1; y++) {
    for (const region of regionsAt(y) ?? []) {
      const start = matcher.starts(region.text).at(-1);
      if (!start || claimed.some(range => range.start.y === y + 1
        && region.xs[start.start]! + 1 <= range.end.x
        && region.xs[start.end - 1]! + 1 >= range.start.x)) continue;
      const gap = edgeGap(region, start.end);
      if (gap < 0 || gap > EDGE_SLACK) continue;
      const margin = region.xs[region.text.search(/\S/)]!;
      const pieces = [{ region, from: start.start, to: start.end }];
      let joined = region.text.slice(start.start, start.end);
      for (let next = y + 1; next <= y + MAX_CHAIN_ROWS && joined.length <= matcher.maxLength; next++) {
        const below = regionsAt(next)?.find(r => r.left === region.left && r.right === region.right);
        if (!below) break;
        const from = below.text.search(/\S/);
        if (from < 0 || below.xs[from]! < margin || below.xs[from]! > margin + INDENT_SLACK) break;
        const run = matcher.continuation(below.text.slice(from), joined);
        if (!run) break;
        pieces.push({ region: below, from, to: from + run.length });
        joined += run;
        const gap = edgeGap(below, from + run.length);
        if (gap < 0 || gap > EDGE_SLACK) break;
      }
      if (pieces.length < 2 || joined.length > matcher.maxLength) continue;
      const link = matcher.find(joined)[0];
      if (!link) continue;
      // Map the parsed link back onto the pieces, excluding quotes and trailing punctuation.
      let offset = 0;
      const ranges: IBufferRange[] = [];
      for (const piece of pieces) {
        const length = piece.to - piece.from;
        const from = Math.max(0, link.start - offset), to = Math.min(length, link.end - offset);
        if (from < to) ranges.push({
          start: { x: piece.region.xs[piece.from + from]! + 1, y: piece.region.y + 1 },
          end: { x: piece.region.ends[piece.from + to - 1]!, y: piece.region.y + 1 },
        });
        offset += length;
      }
      if (ranges.length >= 2 && ranges[0]!.start.y === y + 1) {
        claimed.push(...ranges);
        for (const range of ranges) if (range.start.y === row) found.push({ link, range });
      }
    }
  }
  return found;
}

/** Links in the soft-wrapped logical line containing the 1-based buffer row. */
function plainLinks<T extends TextRange>(terminal: Pick<Terminal, 'buffer'>, row: number, matcher: LinkMatcher<T>): RangedLink<T>[] {
  const buffer = terminal.buffer.active;
  let first = row - 1;
  while (first > 0 && buffer.getLine(first)?.isWrapped) first--;
  let last = row - 1;
  while (buffer.getLine(last + 1)?.isWrapped) last++;
  let text = '';
  const cells: { x: number; y: number; width: number }[] = [];
  for (let y = first; y <= last; y++) {
    const line = buffer.getLine(y);
    if (!line) break;
    for (let x = 0; x < line.length; x++) {
      const cell = line.getCell(x);
      if (!cell || cell.getWidth() === 0) continue;
      const chars = cell.getChars();
      // xterm leaves an empty last cell when a wide character must wrap to the next row.
      if (!chars && x === line.length - 1 && y < last && buffer.getLine(y + 1)?.getCell(0)?.getWidth() === 2) continue;
      text += chars || ' ';
      for (let i = 0; i < (chars.length || 1); i++) cells.push({ x: x + 1, y: y + 1, width: cell.getWidth() });
    }
  }
  return matcher.find(text).flatMap(link => {
    const start = cells[link.start], end = cells[link.end - 1];
    return start && end && start.y <= row && end.y >= row ? [{ link, range: {
      start: { x: start.x, y: start.y }, end: { x: end.x + end.width - 1, y: end.y },
    } }] : [];
  });
}

/** Prefer a complete hard-broken target over partial matches on the same cells. */
export function terminalLinkRanges<T extends TextRange>(terminal: Pick<Terminal, 'buffer'>, row: number, matcher: LinkMatcher<T>): RangedLink<T>[] {
  const columns = columnLinks(terminal, row, matcher);
  const overlaps = (range: IBufferRange) => columns.some(column =>
    (range.start.y < row ? 1 : range.start.x) <= column.range.end.x
    && (range.end.y > row ? Infinity : range.end.x) >= column.range.start.x);
  return [...plainLinks(terminal, row, matcher).filter(link => !overlaps(link.range)), ...columns];
}
