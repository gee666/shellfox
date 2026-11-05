import { describe, expect, it, vi } from 'vitest';
import { prepareFileHtml } from './file-html-preview';
const data = (mime: string, text: string) => `data:${mime};base64,${btoa(text)}`;

describe('project HTML asset embedding', () => {
  it('embeds scripts, images, nested CSS assets and styles before sandboxing', async () => {
    const assets: Record<string, string> = {
      'app.js': data('text/javascript', 'document.title="works"'),
      'styles/main.css': data('text/css', '@import "colors.css"; body { background:url(../image.png) }'),
      'styles/colors.css': data('text/css', '.title{color:red}'),
      'styles/../image.png': data('image/png', 'pixels'),
      'image.png': data('image/png', 'pixels'),
    };
    const load = vi.fn(async (path: string) => assets[path] ?? null);
    const html = await prepareFileHtml('<base href="file:///etc/"><meta http-equiv="refresh" content="0;url=file:///etc/passwd"><link rel="stylesheet" href="styles/main.css"><script src="app.js"></script><img src="image.png"><p style="background:url(image.png)">Hello</p>', load);
    expect(html).toContain(assets['app.js']);
    expect(html).toContain(assets['image.png']);
    expect(decodeURIComponent(html)).toContain('.title{color:red}');
    expect(decodeURIComponent(html)).toContain('background:url(');
    expect(html).not.toContain('file:///etc');
    expect(html).not.toContain('http-equiv="refresh"');
    expect(load).toHaveBeenCalledWith('styles/colors.css');
    expect(load).toHaveBeenCalledWith('styles/../image.png');
    expect(load.mock.calls.filter(([path]) => path === 'image.png')).toHaveLength(1);
  });
  it('never requests external URLs or absolute paths and bounds asset requests', async () => {
    const load = vi.fn(async (_path: string) => data('image/png', 'pixel'));
    const html = '<img src="https://example.com/x"><img src="file:///etc/passwd"><img src="/etc/passwd">'
      + Array.from({ length: 110 }, (_, i) => `<img src="${i}.png">`).join('');
    await prepareFileHtml(html, load);
    expect(load).toHaveBeenCalledTimes(100);
    expect(load.mock.calls.some(args => String(args[0]).includes('passwd'))).toBe(false);
  });
  it('renders the remainder when assets fail and strips a source-provided CSP', async () => {
    const html = await prepareFileHtml('<meta http-equiv="Content-Security-Policy" content="default-src *"><script src="missing.js"></script><h1>Still here</h1>', async () => { throw new Error('missing'); });
    expect(html).toContain('<h1>Still here</h1>');
    expect(html).toContain('src="data:,"');
    expect(html).not.toContain('default-src *');
  });
});
