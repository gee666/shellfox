import type { ILink, ILinkProvider, Terminal } from '@xterm/xterm';
import { findTerminalFileLinks, findTerminalFileLinkStarts, type TerminalFileTarget } from './terminal-file-links';
import { terminalLinkRanges } from './terminal-link-ranges';

function continuation(text: string, joined: string): string | undefined {
  const quote = joined[0];
  if (quote === '"' || quote === "'") {
    if (joined.length > 1 && joined.endsWith(quote)) return undefined;
    const end = text.indexOf(quote);
    return end < 0 ? text.trimEnd() : text.slice(0, end + 1);
  }
  const run = /^[^\s<>"'`|=()\[\]{};,]+/.exec(text)?.[0];
  // A new explicit path or URI is a separate target, not the tail of this one.
  if (!run || /^(?:[a-z][a-z\d+.-]*:(?!\d)|\/|~[\\/]|\.{1,2}[\\/])/i.test(run)) return undefined;
  if (text.slice(run.length).trim() && !/[\\/.:]/.test(run)) return undefined;
  return run;
}

/** File and folder paths in soft wraps, TUI table columns and hard-broken paragraphs. */
export function fileLinkProvider(terminal: Pick<Terminal, 'buffer'>, open: (target: TerminalFileTarget, event: MouseEvent) => void,
  hooks: { hover?(text: string): void; leave?(): void } = {}): ILinkProvider {
  return { provideLinks(row, callback) {
    const links = terminalLinkRanges(terminal, row, {
      find: findTerminalFileLinks, starts: findTerminalFileLinkStarts, continuation, maxLength: 8192,
    }).map(({ link, range }): ILink => ({
      text: link.text, range, decorations: { underline: true, pointerCursor: true },
      activate: event => { if (event.button === 0) open(link, event); },
      hover: () => hooks.hover?.(link.text), leave: () => hooks.leave?.(),
    }));
    callback(links.length ? links : undefined);
  } };
}
