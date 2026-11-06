// Compose existing native screenshots only. No application pixels are generated or edited.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from '@playwright/test';

const root = fileURLToPath(new URL('../../', import.meta.url));
const captures = JSON.parse(readFileSync(process.argv[2] ?? path.join(root, 'tmp/mobile-file-transfers/preview-captures.json'), 'utf8'));
const panels = Array.isArray(captures) ? captures : [['directory', 'Files'], ['code', 'Code'], ['image', 'Image'], ['download', 'Downloads']].map(([key, label]) => ({ path: captures[key], label }));
if (Array.isArray(captures)) assert(panels.length >= 9, 'A redesign review needs directory/code/image native captures at 320dp, 360dp and landscape.');
const escape = text => text.replace(/[&<>"']/g, value => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[value]);
const cards = panels.map(panel => {
  const png = readFileSync(panel.path), width = png.readUInt32BE(16), height = png.readUInt32BE(20);
  return `<section><h2>${escape(panel.label)}</h2><img alt="${escape(panel.label)}" width="${width}" height="${height}" src="data:image/png;base64,${png.toString('base64')}"></section>`;
}).join('');
const browser = await chromium.launch({ headless: true, ...(process.env.SHELLFOX_CHROMIUM_CHANNEL ? { channel: process.env.SHELLFOX_CHROMIUM_CHANNEL } : {}) });
try {
  const page = await browser.newPage({ viewport: { width: panels.length > 4 ? 1024 : 760, height: 900 }, deviceScaleFactor: 1 });
  await page.setContent(`<!doctype html><html><head><meta charset="utf-8"><style>*{box-sizing:border-box}body{margin:0;padding:12px;background:#e9e9ec;color:#17171b;font:14px Arial,sans-serif}h1{font-size:18px;margin:0 0 10px}main{display:grid;grid-template-columns:repeat(${panels.length > 4 ? 3 : 2},minmax(0,1fr));gap:10px;align-items:start}section{min-width:0;border:1px solid #777}h2{margin:0;padding:8px;font-size:13px;background:#f7f7f8}img{display:block;width:100%;height:auto}</style></head><body><h1>Shellfox Files, Android native captures</h1><main>${cards}</main></body></html>`);
  await page.locator('img').evaluateAll(images => Promise.all(images.map(image => image.decode())));
  const output = path.resolve(process.argv[3] ?? path.join(root, 'tmp/mobile-ui-redesign-preview.png'));
  await page.screenshot({ path: output, fullPage: true }); console.log(output);
} finally { await browser.close(); }
