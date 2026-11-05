import { chromium } from '@playwright/test';
import { expect, it } from 'vitest';
import { filePreviewDocument } from './file-preview-document';

it('wraps highlighted code without changing source and restores horizontal scrolling when unwrapped', async () => {
  const browser = await chromium.launch({ headless: true });
  const source = `const long = "${'x'.repeat(1500)}";\n// unchanged`;
  try {
    const page = await browser.newPage({ viewport: { width: 360, height: 600 } });
    const heights: number[] = [];
    for (const wrap of [true, false]) {
      await page.setContent(filePreviewDocument(source, 'code', '#121212', '#eeeeee', undefined, 'file.ts', wrap));
      const frame = page.frames().find(child => child !== page.mainFrame());
      if (!frame) throw new Error('Missing code preview');
      const code = frame.locator('pre code'); await code.waitFor();
      expect(await code.textContent()).toBe(source);
      const layout = await code.evaluate(element => ({ whitespace: getComputedStyle(element).whiteSpace,
        overflowWrap: getComputedStyle(element).overflowWrap, height: element.getBoundingClientRect().height,
        width: element.parentElement?.clientWidth ?? 0, scrollWidth: element.parentElement?.scrollWidth ?? 0 }));
      expect(layout).toMatchObject({ whitespace: wrap ? 'pre-wrap' : 'pre', overflowWrap: wrap ? 'anywhere' : 'normal' });
      if (wrap) expect(layout.scrollWidth).toBe(layout.width); else expect(layout.scrollWidth).toBeGreaterThan(layout.width * 2);
      heights.push(layout.height);
    }
    expect(heights[0]).toBeGreaterThan(heights[1] * 2);
  } finally { await browser.close(); }
}, 20_000);
