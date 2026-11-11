/** File targets from terminal output. Web/custom schemes are never file targets. */
export interface TerminalFileTarget { path: string; line?: number; column?: number }
export interface TerminalFileLink extends TerminalFileTarget { start: number; end: number; text: string }

export function parseTerminalFileTarget(value: string): TerminalFileTarget | null {
  let path = value.trim();
  if (!path || path.length > 8192 || /[\x00-\x1f\x7f]/.test(path)) return null;
  let line: number | undefined;
  let column: number | undefined;
  if (/^file:/i.test(path)) {
    try {
      const url = new URL(path);
      if (url.protocol !== 'file:' || (url.hostname && url.hostname !== 'localhost') || url.search) return null;
      if (url.hash) {
        const source = /^#L?(\d+)(?:(?:C|:)(\d+))?(?:-L?\d+(?:C\d+)?)?$/i.exec(url.hash);
        if (!source) return null;
        line = Number(source[1]); column = source[2] ? Number(source[2]) : undefined;
      }
      path = decodeURIComponent(url.pathname);
      if (/^\/[a-z]:\//i.test(path)) path = path.slice(1);
    } catch { return null; }
  } else if (/^[a-z][a-z\d+.-]*:/i.test(path) && !/^[a-z]:[\\/]/i.test(path)) return null;
  const position = /:(\d+)(?::(\d+))?$/.exec(path);
  if (position) {
    line ??= Number(position[1]); column ??= position[2] ? Number(position[2]) : undefined;
    path = path.slice(0, position.index);
  }
  if ((line !== undefined && (!Number.isSafeInteger(line) || line < 1)) || (column !== undefined && (!Number.isSafeInteger(column) || column < 1))) return null;
  if (!path || /[\x00-\x1f\x7f]/.test(path)) return null;
  // Explicit paths and relative folders with a trailing separator may be extensionless.
  if (!/^(?:[a-z]:[\\/]|\/|~[\\/]|\.{1,2}[\\/])/i.test(path)
    && !/^(?:[^\s<>:"'`|?*]+[\\/])+$/.test(path)
    && !/^(?:[^\s<>:"'`|?*]+[\\/])*[^\s<>:"'`|?*]+\.[a-z\d][a-z\d._+-]*$/i.test(path)) return null;
  return { path, ...(line ? { line } : {}), ...(column ? { column } : {}) };
}

function fileTokens(text: string, incomplete = false) {
  // At a column edge, keep an unfinished quote together, but not an apostrophe within prose.
  const tokens = incomplete
    ? /(?<!\w)"[^"\r\n]*(?:"|$)|(?<!\w)'[^'\r\n]*(?:'|$)|[^\s<>"'`|=()\[\]{};,│┃║┆┇┊┋╎╏]+/g
    : /"[^"\r\n]*"|'[^'\r\n]*'|[^\s<>"'`|=()\[\]{};,│┃║┆┇┊┋╎╏]+/g;
  return text.matchAll(tokens);
}

/** Candidate starts may be incomplete paths or source-position suffixes at a column edge. */
export function findTerminalFileLinkStarts(text: string): { start: number; end: number }[] {
  const starts: { start: number; end: number }[] = [];
  for (const match of fileTokens(text, true)) {
    const raw = match[0].trimEnd(), token = raw.replace(/^["']|["']$/g, '');
    if (/^[a-z][a-z\d+.-]*:(?!\d)/i.test(token) && !/^(?:file:|[a-z]:[\\/])/i.test(token)) continue;
    if (!parseTerminalFileTarget(token.replace(/:\d*(?::\d*)?$/, '')) && !/^(?:file:|[a-z]:[\\/]|\/|~[\\/]|\.{1,2}[\\/]|[^\s<>:"'`|?*]+[\\/])/i.test(token)) continue;
    starts.push({ start: match.index, end: match.index + raw.length });
  }
  return starts;
}

export function findTerminalFileLinks(text: string): TerminalFileLink[] {
  const found: TerminalFileLink[] = [];
  for (const match of fileTokens(text)) {
    const raw = match[0], quoted = raw[0] === '"' || raw[0] === "'";
    const token = (quoted ? raw.slice(1, -1) : raw).replace(/[.!?]+$/, '');
    const target = parseTerminalFileTarget(token);
    if (!target) continue;
    const start = match.index + (quoted ? 1 : 0);
    found.push({ ...target, start, end: start + token.length, text: token });
  }
  return found;
}
