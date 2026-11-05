import { marked } from 'marked';
import { fileSourceLanguage, fileSourcePreview } from './file-source-preview';

export type TextPreviewKind = 'markdown' | 'html' | 'svg' | 'code' | 'text';
export function textPreviewKind(path: string, mimeType: string): TextPreviewKind {
  if (/\.(?:md|mdx|markdown|mdown|mkd)$/i.test(path) || mimeType === 'text/markdown') return 'markdown';
  if (/\.(?:html?|xhtml)$/i.test(path) || mimeType === 'text/html') return 'html';
  if (/\.svg$/i.test(path) || mimeType === 'image/svg+xml') return 'svg';
  return fileSourceLanguage(path) ? 'code' : 'text';
}
export function escapePreviewHtml(value: string): string {
  return value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;').replace(/'/g, '&#39;');
}

export function renderFileMarkdown(text: string): string {
  return marked.parse(text, { async: false, gfm: true });
}

// The child has an opaque origin. It cannot access the wrapper, native APIs,
// host files, cookies, or network resources, even when inline scripts run.
export function filePreviewDocument(text: string, kind: Exclude<TextPreviewKind, 'text'> | 'image', background: string, foreground: string, preparedMarkdown?: string, path = '', wrapLines = true): string {
  const color = (value: string, fallback: string) => /^#[0-9a-f]{6}$/i.test(value) ? value : fallback;
  const bg = color(background, '#121212'), fg = color(foreground, '#eeeeee');
  const scripts = kind === 'html' ? "'unsafe-inline' data:" : "'none'";
  const policy = `default-src 'none'; script-src ${scripts}; style-src 'unsafe-inline' data:; img-src data: blob:; media-src data: blob:; font-src data:; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'`;
  const body = kind === 'markdown' ? preparedMarkdown ?? renderFileMarkdown(text)
    : kind === 'code' ? fileSourcePreview(text, path, wrapLines)
    : kind === 'image' ? `<img alt="Image preview" src="${escapePreviewHtml(text)}">`
    : kind === 'svg' ? `<img alt="SVG preview" src="data:image/svg+xml,${encodeURIComponent(text)}">` : text;
  const child = `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${policy}"><meta name="viewport" content="width=device-width, initial-scale=1"><style>html{color-scheme:dark light;background:${bg};color:${fg};font:15px system-ui;overflow-wrap:anywhere}body{margin:16px}pre{overflow:auto;padding:10px;background:#8882}code{font-family:monospace}img,video{max-width:100%;height:auto}table{border-collapse:collapse}td,th{border:1px solid #8886;padding:5px}a{color:inherit}blockquote{border-left:2px solid #888;padding-left:12px;margin-left:0}</style></head><body>${body}</body></html>`;
  return `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; frame-src about:; style-src 'unsafe-inline' data:; script-src ${scripts}; img-src data: blob:; media-src data: blob:; font-src data:; connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'"><meta name="viewport" content="width=device-width, initial-scale=1"><style>html,body,iframe{margin:0;padding:0;width:100%;height:100%;border:0;background:${bg}}iframe{display:block}</style></head><body><iframe title="File preview" sandbox="${kind === 'html' ? 'allow-scripts' : ''}" referrerpolicy="no-referrer" srcdoc="${escapePreviewHtml(child)}"></iframe></body></html>`;
}

export function allowFilePreviewNavigation(url: string): boolean {
  return url === 'about:blank' || url === 'about:srcdoc';
}

export function fileParentPath(path: string): string {
  const normalized = path.replace(/\\/g, '/').replace(/\/+$/, '');
  const index = normalized.lastIndexOf('/');
  if (index < 0) return '.';
  if (index === 0) return '/';
  const parent = normalized.slice(0, index);
  return /^[a-z]:$/i.test(parent) ? `${parent}/` : parent;
}

export function sourceSelection(text: string, line = 1, column = 1): { start: number; end: number } {
  const lines = text.split('\n');
  const row = Math.max(0, Math.min(lines.length - 1, line - 1));
  let start = 0;
  for (let index = 0; index < row; index++) start += lines[index].length + 1;
  start += Math.max(0, Math.min(lines[row].replace(/\r$/, '').length, column - 1));
  return { start, end: start };
}
