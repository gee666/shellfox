import { chromium } from '@playwright/test';
import { expect, it } from 'vitest';
import { prepareFileHtml } from '../../src/shared/file-html-preview';
import { filePreviewDocument, renderFileMarkdown } from './file-preview-document';

it('runs inline and sibling HTML scripts in an opaque frame without native, parent, storage or network access', async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage();
    const external: string[] = [];
    page.on('request', request => { if (/^(?:https?|file):/.test(request.url())) external.push(request.url()); });
    await page.route('https://**/*', route => route.abort());
    await page.evaluate(() => { Object.assign(window, { secret: 'private', ReactNativeWebView: { postMessage: () => { throw new Error('Native bridge reached'); } } }); });
    const html = await prepareFileHtml(`<button id="run" onclick="this.textContent=String(window.sibling+1)">Run</button>
      <script src="sibling.js"></script><script>
      document.body.dataset.bridge=String(typeof window.ReactNativeWebView);
      try { document.body.dataset.parent=parent.secret; } catch { document.body.dataset.parent='blocked'; }
      try { localStorage.setItem('x','x'); document.body.dataset.storage='allowed'; } catch { document.body.dataset.storage='blocked'; }
      fetch('https://example.invalid/leak').then(()=>document.body.dataset.network='allowed').catch(()=>document.body.dataset.network='blocked');
      </script>`, async asset => asset === 'sibling.js' ? `data:text/javascript;base64,${btoa('window.sibling=41;')}` : null);
    await page.setContent(filePreviewDocument(html, 'html', '#121212', '#eeeeee'));
    const child = page.frames().find(frame => frame !== page.mainFrame());
    expect(child).toBeDefined();
    if (!child) throw new Error('Missing preview frame');
    await child.locator('#run').click();
    expect(await child.locator('#run').textContent()).toBe('42');
    await child.waitForFunction(() => document.body.dataset.network === 'blocked');
    expect(await child.locator('body').getAttribute('data-parent')).toBe('blocked');
    expect(await child.locator('body').getAttribute('data-storage')).toBe('blocked');
    expect(await child.locator('body').getAttribute('data-bridge')).toBe('undefined');
    expect(external).toEqual([]);
    await page.close();
  } finally { await browser.close(); }
}, 20_000);

it('renders Markdown and SVG without running embedded scripts', async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    for (const kind of ['markdown', 'svg'] as const) {
      const page = await browser.newPage();
      const text = kind === 'markdown' ? '# Heading\n\n![Sibling](diagram.svg)\n<script>document.body.dataset.attack="yes"</script>'
        : '<svg xmlns="http://www.w3.org/2000/svg"><script>document.body.dataset.attack="yes"</script><rect width="10" height="10"/></svg>';
      const markdown = kind === 'markdown' ? await prepareFileHtml(renderFileMarkdown(text), async path =>
        path === 'diagram.svg' ? `data:image/svg+xml;base64,${btoa('<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>')}` : null) : undefined;
      await page.setContent(filePreviewDocument(text, kind, '#121212', '#eeeeee', markdown));
      const child = page.frames().find(frame => frame !== page.mainFrame());
      if (!child) throw new Error('Missing preview frame');
      await child.locator(kind === 'markdown' ? 'h1' : 'img').waitFor();
      if (kind === 'markdown') {
        await child.waitForFunction(() => document.querySelector('img')?.naturalWidth === 10);
        expect(await child.locator('h1').textContent()).toBe('Heading');
      }
      expect(await child.locator('body').getAttribute('data-attack')).toBeNull();
      await page.close();
    }
  } finally { await browser.close(); }
}, 20_000);
