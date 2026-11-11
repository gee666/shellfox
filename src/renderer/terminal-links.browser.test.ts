import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';
import { build } from 'esbuild';
import { expect, it } from 'vitest';

it('opens complete file and folder targets when clicking every wrapped row in xterm', async () => {
  const bundle = await build({
    stdin: { resolveDir: process.cwd(), loader: 'ts', contents: `
      import { Terminal } from '@xterm/xterm';
      import { fileLinkProvider } from './src/shared/terminal-file-provider';
      const terminal = new Terminal({ cols: 40, rows: 8, fontSize: 14 });
      terminal.open(document.getElementById('terminal'));
      const result = document.getElementById('result');
      terminal.registerLinkProvider(fileLinkProvider(terminal, ({ path, line, column }) => {
        result.textContent = JSON.stringify({ path, line, column });
      }));
      window.addEventListener('message', event => {
        result.textContent = '';
        result.dataset.ready = 'no';
        terminal.reset();
        terminal.resize(event.data.cols, 8);
        terminal.write(event.data.text, () => { result.dataset.ready = 'yes'; });
      });
    ` },
    bundle: true, write: false, format: 'iife', platform: 'browser',
  });
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage({ viewport: { width: 800, height: 400 } });
    await page.setContent('<div id="terminal"></div><output id="result"></output>');
    await page.addStyleTag({ content: await readFile('node_modules/@xterm/xterm/css/xterm.css', 'utf8') });
    await page.addScriptTag({ content: bundle.outputFiles[0]!.text });
    const result = page.locator('#result');
    for (const { text, cols, target, cells } of [
      {
        text: '│ ./src/very-long- │ other │\r\n│ folder/a.ts:12:3 │ text  │', cols: 40,
        target: { path: './src/very-long-folder/a.ts', line: 12, column: 3 }, cells: [[4, 1], [4, 2]],
      },
      {
        text: '│ C:\\work\\long-  │ other │\r\n│ folder\\subdir\\ │ text  │', cols: 40,
        target: { path: 'C:\\work\\long-folder\\subdir\\' }, cells: [[4, 1], [4, 2]],
      },
      {
        text: './src/very-long-folder/a.ts:12:3', cols: 16,
        target: { path: './src/very-long-folder/a.ts', line: 12, column: 3 }, cells: [[4, 1], [4, 2]],
      },
    ]) {
      await page.mouse.move(790, 390);
      await page.evaluate(data => window.dispatchEvent(new MessageEvent('message', { data })), { text, cols });
      await result.waitFor({ state: 'attached' });
      await expect.poll(() => result.getAttribute('data-ready')).toBe('yes');
      const box = await page.locator('.xterm-screen').boundingBox();
      if (!box) throw new Error('Missing terminal screen');
      for (const [x, y] of cells) {
        await result.evaluate(element => { element.textContent = ''; });
        const clientX = box.x + box.width / cols * (x! - 0.5), clientY = box.y + box.height / 8 * (y! - 0.5);
        await page.mouse.move(clientX, clientY);
        await expect.poll(() => page.locator('.xterm-screen').getAttribute('class')).toContain('xterm-cursor-pointer');
        await page.mouse.click(clientX, clientY);
        await expect.poll(async () => JSON.parse(await result.textContent() || 'null')).toEqual(target);
      }
      if (cols === 40) {
        await result.evaluate(element => { element.textContent = ''; });
        await page.mouse.move(box.x + box.width / cols * 22, box.y + box.height / 16);
        await page.mouse.click(box.x + box.width / cols * 22, box.y + box.height / 16);
        expect(await result.textContent()).toBe('');
      }
    }
  } finally { await browser.close(); }
}, 20_000);
