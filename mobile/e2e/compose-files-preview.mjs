import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from '@playwright/test';

const root = fileURLToPath(new URL('../../', import.meta.url));
const captures = JSON.parse(readFileSync(process.argv[2] ?? path.join(root, 'tmp/mobile-file-transfers/preview-captures.json'), 'utf8'));
const panels = [['directory', 'Files · host home'], ['code', 'Code · soft wrap'], ['image', 'Image · touch crop'], ['download', 'Download · saved in Downloads']];
const cards = panels.map(([key, caption]) => `<section><h2>${caption}</h2><img alt="${caption}" src="data:image/png;base64,${readFileSync(captures[key]).toString('base64')}"></section>`).join('');
const browser = await chromium.launch({ headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 760, height: 1780 }, deviceScaleFactor: 1 });
  await page.setContent(`<!doctype html><html><head><meta charset="utf-8"><style>*{box-sizing:border-box}body{margin:0;padding:16px;background:#e9e9ec;color:#17171b;font:15px Arial,sans-serif}h1{font-size:18px;margin:0 0 12px}main{display:grid;grid-template-columns:1fr 1fr;gap:12px}section{min-width:0;background:#111016;border:1px solid #777}h2{height:36px;margin:0;padding:10px 8px;font-size:14px;background:#f7f7f8}img{display:block;width:100%;height:802px;object-fit:contain;object-position:top}</style></head><body><h1>Shellfox mobile · Android 35 · native captures</h1><main>${cards}</main></body></html>`);
  await page.locator('img').evaluateAll(images => Promise.all(images.map(image => image.decode())));
  const output = path.join(root, 'tmp/files-refine-mobile-preview.png');
  await page.screenshot({ path: output, fullPage: true });
  console.log(output);
} finally { await browser.close(); }
