import { JSDOM } from 'jsdom';
import { expect, it } from 'vitest';
import { fileSourceLanguage, fileSourcePreview } from './file-source-preview';
import { filePreviewDocument } from './file-preview-document';

it.each([
  ['sample.js', 'const answer = 42;', 'javascript'], ['sample.ts', 'const answer: number = 42;', 'typescript'],
  ['sample.tsx', 'const view = <span>text</span>;', 'tsx'], ['sample.json', '{"answer":42}', 'json'],
  ['sample.xml', '<root enabled="yes">text</root>', 'markup'], ['sample.py', 'def hello():\n  return 42', 'python'],
  ['sample.ps1', '$value = Get-ChildItem', 'powershell'], ['sample.cs', 'public class Example {}', 'csharp'],
  ['sample.yaml', 'value: true', 'yaml'], ['sample.css', 'body { color: red; }', 'css'],
])('highlights %s using a fixed grammar without changing source text', (path, source, language) => {
  expect(fileSourceLanguage(path)).toBe(language);
  const document = new JSDOM(fileSourcePreview(source, path)).window.document;
  expect(document.querySelectorAll('span.token').length).toBeGreaterThan(0);
  expect(document.querySelector('code')?.textContent).toBe(source);
});
it('escapes hostile markup and renders code in a script-free sandbox', () => {
  const source = '</code></pre><img src=x onerror=alert(1)><script>attack()</script>';
  const wrapper = new JSDOM(filePreviewDocument(source, 'code', '#121212', '#eeeeee', undefined, 'sample.xml'));
  const frame = wrapper.window.document.querySelector('iframe')!;
  const child = new JSDOM(frame.getAttribute('srcdoc')!).window.document;
  expect(frame.getAttribute('sandbox')).toBe('');
  expect(child.querySelectorAll('img,script')).toHaveLength(0);
  expect(child.querySelector('code')?.textContent).toBe(source);
  expect(child.querySelector('meta[http-equiv]')?.getAttribute('content')).toContain("script-src 'none'");
});
it('keeps minified and unsupported text readable without expensive tokenization', () => {
  for (const [path, source] of [['large.js', '<'.repeat(10_001)], ['notes.txt', '<b>literal</b>']]) {
    const document = new JSDOM(fileSourcePreview(source, path)).window.document;
    expect(document.querySelectorAll('span.token')).toHaveLength(0);
    expect(document.querySelector('code')?.textContent).toBe(source);
    expect(document.querySelector('b')).toBeNull();
  }
});
