import { describe, expect, it } from 'vitest';
import { JSDOM } from 'jsdom';
import { allowFilePreviewNavigation, fileParentPath, filePreviewDocument, sourceSelection, textPreviewKind } from './file-preview-document';

function childDocument(text: string, kind: 'markdown' | 'html' | 'svg') {
  const wrapper = new JSDOM(filePreviewDocument(text, kind, '#121212', '#eeeeee'));
  const frame = wrapper.window.document.querySelector('iframe')!;
  return { wrapper, frame, child: new JSDOM(frame.getAttribute('srcdoc')!) };
}
describe('isolated mobile file preview documents', () => {
  it('renders Markdown headings, code, lists and tables without granting script permission', () => {
    const { frame, child } = childDocument('# Heading\n\n**Bold**\n\n- list\n\n```ts\nconst n = 1;\n```\n\n| Name |\n| --- |\n| value |', 'markdown');
    expect(frame.getAttribute('sandbox')).toBe('');
    expect(child.window.document.querySelector('h1')?.textContent).toBe('Heading');
    expect(child.window.document.querySelector('strong')?.textContent).toBe('Bold');
    expect(child.window.document.querySelector('code')?.textContent).toContain('const n = 1;');
    expect(child.window.document.querySelector('table')).not.toBeNull();
    expect(child.window.document.querySelector('meta[http-equiv]')?.getAttribute('content')).toContain("script-src 'none'");
  });
  it('isolates interactive HTML with an opaque sandbox and deny-by-default CSP', () => {
    const { wrapper, frame, child } = childDocument('<button onclick="this.textContent=123">Run</button><script>parent.postMessage("attack", "*")</script>', 'html');
    expect(frame.getAttribute('sandbox')).toBe('allow-scripts');
    expect(frame.getAttribute('sandbox')).not.toContain('allow-same-origin');
    expect(frame.getAttribute('referrerpolicy')).toBe('no-referrer');
    expect(wrapper.window.document.querySelectorAll('script')).toHaveLength(0);
    const policy = child.window.document.querySelector('meta[http-equiv]')?.getAttribute('content');
    for (const directive of ["default-src 'none'", "connect-src 'none'", "script-src 'unsafe-inline' data:", "style-src 'unsafe-inline' data:", "base-uri 'none'", "form-action 'none'", "frame-src 'none'"]) expect(policy).toContain(directive);
    expect(child.window.document.body.innerHTML).toContain('onclick=');
  });
  it('keeps SVG inside an image, with no scripts or same-origin permissions', () => {
    const { frame, child } = childDocument('<svg><script>alert(1)</script></svg>', 'svg');
    expect(frame.getAttribute('sandbox')).toBe('');
    expect(child.window.document.querySelectorAll('script')).toHaveLength(0);
    expect(child.window.document.querySelector('img')?.src).toContain('data:image/svg+xml,');
  });
  it('keeps image data URLs in script-free img documents', () => {
    const doc = new JSDOM(filePreviewDocument('data:image/x-icon;base64,YQ==', 'image', '#121212', '#eeeeee'));
    const frame = doc.window.document.querySelector('iframe')!;
    const child = new JSDOM(frame.getAttribute('srcdoc')!).window.document;
    expect(frame.getAttribute('sandbox')).toBe('');
    expect(child.querySelector('img')?.src).toBe('data:image/x-icon;base64,YQ==');
    expect(child.querySelector('meta[http-equiv]')?.getAttribute('content')).toContain("script-src 'none'");
  });
  it('does not let untrusted text or theme values escape the wrapper', () => {
    const doc = new JSDOM(filePreviewDocument('</iframe><script>attack()</script>', 'html', '"><script>oops()</script>', '#fff'));
    expect(doc.window.document.querySelectorAll('iframe')).toHaveLength(1);
    expect(doc.window.document.querySelectorAll('script')).toHaveLength(0);
  });
  it.each(['file:///private/data', 'https://example.com', 'data:text/html,x', 'intent://open', 'javascript:alert(1)', 'about:blank#other'])('denies navigation to %s', url => {
    expect(allowFilePreviewNavigation(url)).toBe(false);
  });
  it('permits only the inline wrapper and its srcdoc document', () => {
    expect(allowFilePreviewNavigation('about:blank')).toBe(true);
    expect(allowFilePreviewNavigation('about:srcdoc')).toBe(true);
  });
});
it('finds host parent paths without using the phone OS path rules', () => {
  expect(fileParentPath('/work/src/file.ts')).toBe('/work/src');
  expect(fileParentPath('/file.ts')).toBe('/');
  expect(fileParentPath('C:\\work\\file.ts')).toBe('C:/work');
  expect(fileParentPath('C:\\file.ts')).toBe('C:/');
  expect(fileParentPath('file.ts')).toBe('.');
});
it('selects a source position and clamps out-of-range terminal line/column suffixes', () => {
  expect(sourceSelection('one\ntwo\nthree', 2, 2)).toEqual({ start: 5, end: 5 });
  expect(sourceSelection('one\r\ntwo', 1, 100)).toEqual({ start: 3, end: 3 });
  expect(sourceSelection('one\ntwo', 999, 999)).toEqual({ start: 7, end: 7 });
});
it('detects previewable text without relying solely on the host MIME type', () => {
  expect(textPreviewKind('README.MD', 'text/plain')).toBe('markdown');
  expect(textPreviewKind('page.mdx', 'text/plain')).toBe('markdown');
  expect(textPreviewKind('page.xhtml', 'application/xhtml+xml')).toBe('html');
  expect(textPreviewKind('index.html', 'text/plain')).toBe('html');
  expect(textPreviewKind('icon.svg', 'image/svg+xml')).toBe('svg');
  expect(textPreviewKind('index.ts', 'text/plain')).toBe('code');
  expect(textPreviewKind('notes.txt', 'text/plain')).toBe('text');
});
