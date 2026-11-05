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
  // Absolute paths and explicit relative paths may be extensionless.
  if (!/^(?:[a-z]:[\\/]|\/|~[\\/]|\.{1,2}[\\/])/i.test(path)
    && !/^(?:[^\s<>:"'`|?*]+[\\/])*[^\s<>:"'`|?*]+\.[a-z\d][a-z\d._+-]*$/i.test(path)) return null;
  return { path, ...(line ? { line } : {}), ...(column ? { column } : {}) };
}

export function findTerminalFileLinks(text: string): TerminalFileLink[] {
  const found: TerminalFileLink[] = [];
  // Quoted paths preserve spaces. Otherwise stop at shell/prose delimiters.
  const tokens = /"([^"\r\n]+)"|'([^'\r\n]+)'|[^\s<>"'`|=()\[\]{};,]+/g;
  for (const match of text.matchAll(tokens)) {
    const quoted = match[1] ?? match[2];
    const token = (quoted ?? match[0]).replace(/[.!?]+$/, '');
    const target = parseTerminalFileTarget(token);
    if (!target) continue;
    const start = match.index + (quoted !== undefined ? 1 : 0);
    found.push({ ...target, start, end: start + token.length, text: token });
  }
  return found;
}
