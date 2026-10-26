/** Compare completion text without interpreting paths or expanding the host's home directory. */
export function normalizeDirectoryCompletionPath(input: string, separator?: '/' | '\\'): string {
  const trimmed = input.trim();
  const unquoted = trimmed.length >= 2 && (trimmed[0] === '"' || trimmed[0] === "'") && trimmed.at(-1) === trimmed[0]
    ? trimmed.slice(1, -1) : input;
  return unquoted.replace(separator === '/' ? /\/+$/ : /[\\/]+$/, '');
}

/** An exact directory must not hide other matches such as work/ and workshop/. */
export function visibleDirectoryCompletions(input: string, matches: readonly string[]): readonly string[] {
  const only = matches[0];
  if (matches.length !== 1 || only === undefined) return matches;
  // Hosts append their separator. A POSIX folder may itself end in a backslash.
  const separator = only.endsWith('/') ? '/' : only.endsWith('\\') ? '\\' : undefined;
  return normalizeDirectoryCompletionPath(input, separator) === normalizeDirectoryCompletionPath(only, separator) ? [] : matches;
}
