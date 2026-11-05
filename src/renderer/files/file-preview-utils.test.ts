import { describe, expect, it } from 'vitest';
import { compareFileEntries, fileName, isHtmlPreviewFile, isMarkdownPreviewFile, isSvgPreviewFile, maskMarkdownFrontmatter, sourcePosition } from './file-preview-utils';

describe('file preview helpers', () => {
  it('detects supported render modes without depending on host path separators', () => {
    expect(isMarkdownPreviewFile('C:\\work\\README.MDX')).toBe(true);
    expect(isMarkdownPreviewFile('/work/notmd')).toBe(false);
    expect(isHtmlPreviewFile('/work/page.xhtml')).toBe(true);
    expect(isSvgPreviewFile('C:\\work\\art.SVG')).toBe(true);
    expect(fileName('C:\\work\\art.SVG')).toBe('art.SVG');
    expect(fileName('/work/readme.md')).toBe('readme.md');
  });
  it('masks only complete leading metadata and preserves CRLF offsets', () => {
    const source = '\uFEFF---\r\ntitle: note\r\n...\r\n# Title';
    const masked = maskMarkdownFrontmatter(source);
    expect(masked.length).toBe(source.length);
    expect(masked.indexOf('# Title')).toBe(source.indexOf('# Title'));
    expect(masked).not.toContain('title: note');
    expect(maskMarkdownFrontmatter('---\ntitle: incomplete')).toBe('---\ntitle: incomplete');
    expect(maskMarkdownFrontmatter('Text\n---\ntitle: note\n---')).toBe('Text\n---\ntitle: note\n---');
  });
  it('sorts directory names before files and uses natural numeric ordering', () => {
    const entries = [{ name: 'file10', kind: 'file' }, { name: 'Folder', kind: 'directory' }, { name: 'file2', kind: 'file' }];
    expect(entries.sort(compareFileEntries).map(entry => entry.name)).toEqual(['Folder', 'file2', 'file10']);
  });
  it('clamps source positions on LF and CRLF files and excludes CR from column count', () => {
    expect(sourcePosition('a\r\nbc\r\nd', 2, 2)).toBe(4);
    expect(sourcePosition('a\r\nbc\r\nd', 2, 100)).toBe(5);
    expect(sourcePosition('a\nb', 100, 100)).toBe(3);
    expect(sourcePosition('a\nb', -4, -4)).toBe(0);
    expect(sourcePosition('', 100, 100)).toBe(0);
  });
});
