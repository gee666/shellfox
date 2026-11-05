import type { ILink, ILinkProvider, Terminal } from '@xterm/xterm';
import { findTerminalFileLinks, type TerminalFileTarget } from './terminal-file-links';

/** Match file paths in a soft-wrapped logical line, mapping UTF-16 offsets to terminal cells. */
export function fileLinkProvider(terminal: Terminal, open: (target: TerminalFileTarget, event: MouseEvent) => void,
  hooks: { hover?(text: string): void; leave?(): void } = {}): ILinkProvider {
  return { provideLinks(row, callback) {
    const buffer = terminal.buffer.active;
    let first = row - 1;
    while (first > 0 && buffer.getLine(first)?.isWrapped) first--;
    let last = row - 1;
    while (buffer.getLine(last + 1)?.isWrapped) last++;
    let text = '';
    const positions: { x: number; y: number }[] = [];
    for (let y = first; y <= last; y++) {
      const line = buffer.getLine(y);
      if (!line) break;
      for (let x = 0; x < line.length; x++) {
        const cell = line.getCell(x);
        if (!cell || cell.getWidth() === 0) continue;
        const chars = cell.getChars() || ' ';
        text += chars;
        for (let i = 0; i < chars.length; i++) positions.push({ x: x + 1, y: y + 1 });
      }
    }
    const links: ILink[] = [];
    for (const target of findTerminalFileLinks(text)) {
      const start = positions[target.start], end = positions[target.end - 1];
      if (!start || !end || start.y > row || end.y < row) continue;
      links.push({ text: target.text, range: { start, end }, decorations: { underline: true, pointerCursor: true },
        activate: event => { if (event.button === 0) open(target, event); },
        hover: () => hooks.hover?.(target.text), leave: () => hooks.leave?.(),
      });
    }
    callback(links.length ? links : undefined);
  } };
}
