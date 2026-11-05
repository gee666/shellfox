// Portions adapted from T3 Code. Copyright (c) 2026 T3 Tools Inc.
// MIT license and exact upstream paths: ./T3CODE-LICENSE.txt and ./reuse.md.
export const isMarkdownPreviewFile = (path: string): boolean => /\.(?:md|mdx|markdown|mdown|mkd)$/i.test(path);
export const isHtmlPreviewFile = (path: string): boolean => /\.(?:html?|xhtml)$/i.test(path);
export const isSvgPreviewFile = (path: string): boolean => /\.svg$/i.test(path);

const LEADING_FRONTMATTER = /^\uFEFF?---[ \t]*(?:\r\n|\r|\n)(?:[\s\S]*?(?:\r\n|\r|\n))?(?:---|\.\.\.)[ \t]*(?=\r\n|\r|\n|$)/;

/** Preserve source line offsets while omitting YAML metadata from the preview. */
export function maskMarkdownFrontmatter(markdown: string): string {
  return markdown.replace(LEADING_FRONTMATTER, block => block.replace(/[^\r\n]/g, ' '));
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });
export function compareFileEntries(left: { name: string; kind: string }, right: { name: string; kind: string }): number {
  if (left.kind !== right.kind) return left.kind === 'directory' ? -1 : 1;
  return collator.compare(left.name, right.name);
}

export function fileName(path: string): string { return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path; }
export function fileDirectory(path: string): string {
  const separator = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  if (separator < 0) return '.';
  return path.slice(0, separator === 0 || separator === 2 && /^[a-z]:/i.test(path) ? separator + 1 : separator);
}

export function sourcePosition(text: string, line = 1, column = 1): number {
  const lines = text.split('\n');
  const index = Math.max(0, Math.min(lines.length - 1, line - 1));
  let offset = 0;
  for (let current = 0; current < index; current++) offset += lines[current].length + 1;
  return offset + Math.max(0, Math.min(lines[index].replace(/\r$/, '').length, column - 1));
}
