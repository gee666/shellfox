import type { ILink, ILinkProvider, Terminal } from '@xterm/xterm';
import { terminalLinkRanges } from '../shared/terminal-link-ranges';

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

/** Plain-text URLs, including soft wraps and hard breaks inside table columns. */
export function webLinkProvider(terminal: Terminal, open: (url: string, event: MouseEvent) => void,
  hooks: { hover?(url: string): void; leave?(): void } = {}): ILinkProvider {
  return { provideLinks(row, callback) {
    const links = terminalLinkRanges(terminal, row, {
      find: findWebLinks,
      starts: text => [...text.matchAll(/https?:\/\/[^\s<>"'`]+/g)]
        .map(match => ({ start: match.index, end: match.index + match[0].length })),
      continuation: text => {
        const run = /^[^\s<>"'`]+/.exec(text)?.[0];
        if (!run || /^https?:\/\//.test(run)) return undefined;
        // Prose under a complete URL is not a continuation unless the tail looks like a URL.
        if (text.slice(run.length).trim() && !/[/?#=&%~+]/.test(run)) return undefined;
        return run;
      },
      maxLength: MAX_LINK_LENGTH,
    }).map(({ link, range }): ILink => ({
      text: link.url, range, decorations: { underline: true, pointerCursor: true },
      activate: event => { if (event.button === 0) open(link.url, event); },
      hover: () => hooks.hover?.(link.url), leave: () => hooks.leave?.(),
    }));
    callback(links.length ? links : undefined);
  } };
}
