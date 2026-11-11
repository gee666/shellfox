import { readFile } from 'node:fs/promises';
import { chromium, type Browser } from '@playwright/test';
import { build } from 'esbuild';
import { afterAll, beforeAll, expect, it } from 'vitest';
import type { TerminalFileTarget } from '../shared/terminal-file-links';

let browser: Browser, script: string, css: string;
beforeAll(async () => {
  const bundle = await build({
    stdin: { resolveDir: process.cwd(), loader: 'ts', contents: `
      import { Terminal } from '@xterm/xterm';
      import { fileLinkProvider } from './src/shared/terminal-file-provider';
      window.addEventListener('message', event => {
        const terminal = new Terminal({ cols: event.data.cols, rows: 8, fontSize: 14 });
        terminal.open(document.getElementById('terminal'));
        const result = document.getElementById('result');
        terminal.registerLinkProvider(fileLinkProvider(terminal, ({ path, line, column }) => {
          result.textContent = JSON.stringify({ path, line, column });
        }, {
          hover: text => { result.dataset.hovered = text; },
          leave: () => { delete result.dataset.hovered; },
        }));
        terminal.write(event.data.text, () => {
          const rendered = terminal.onRender(({ start, end }) => {
            if (start !== 0 || end !== terminal.rows - 1) return;
            rendered.dispose();
            result.dataset.ready = 'yes';
          });
          terminal.refresh(0, terminal.rows - 1);
        });
      }, { once: true });
    ` },
    bundle: true, write: false, format: 'iife', platform: 'browser',
  });
  script = bundle.outputFiles[0]!.text;
  css = await readFile('node_modules/@xterm/xterm/css/xterm.css', 'utf8');
  browser = await chromium.launch({ headless: true });
});
afterAll(async () => { await browser?.close(); });

const cases: { name: string; text: string; cols: number; target: TerminalFileTarget; cells: [number, number][] }[] = [
  {
    name: 'table file with line and column',
    text: '│ ./src/very-long- │ other │\r\n│ folder/a.ts:12:3 │ text  │', cols: 40,
    target: { path: './src/very-long-folder/a.ts', line: 12, column: 3 }, cells: [[4, 1], [4, 2]],
  },
  {
    name: 'Windows table folder',
    text: '│ C:\\work\\long-  │ other │\r\n│ folder\\subdir\\ │ text  │', cols: 40,
    target: { path: 'C:\\work\\long-folder\\subdir\\' }, cells: [[4, 1], [4, 2]],
  },
  {
    name: 'soft-wrapped file with line and column',
    text: './src/very-long-folder/a.ts:12:3', cols: 16,
    target: { path: './src/very-long-folder/a.ts', line: 12, column: 3 }, cells: [[4, 1], [4, 2]],
  },
];

it.each(cases)('opens the complete $name target from every wrapped row', async ({ text, cols, target, cells }) => {
  // reset() retains xterm's row-link cache. Independent fixtures need independent terminals.
  const page = await browser.newPage({ viewport: { width: 800, height: 400 } });
  try {
    await page.setContent('<div id="terminal"></div><output id="result"></output>');
    await page.addStyleTag({ content: css });
    await page.addScriptTag({ content: script });
    await page.evaluate(data => window.dispatchEvent(new MessageEvent('message', { data })), { text, cols });
    const result = page.locator('#result');
    // A write callback signals parsing, not painting or linkifier refresh.
    await expect.poll(() => result.getAttribute('data-ready')).toBe('yes');
    const box = await page.locator('.xterm-screen').boundingBox();
    if (!box) throw new Error('Missing terminal screen');
    const hovered = `${target.path}${target.line ? `:${target.line}` : ''}${target.column ? `:${target.column}` : ''}`;
    for (const [x, y] of cells) {
      await result.evaluate(element => { element.textContent = ''; });
      const clientX = box.x + box.width / cols * (x - 0.5), clientY = box.y + box.height / 8 * (y - 0.5);
      await page.mouse.move(clientX, clientY);
      await expect.poll(() => result.getAttribute('data-hovered')).toBe(hovered);
      await expect.poll(() => page.locator('.xterm-screen').getAttribute('class')).toContain('xterm-cursor-pointer');
      await page.mouse.click(clientX, clientY);
      await expect.poll(async () => JSON.parse(await result.textContent() || 'null')).toEqual(target);
    }
    if (cols === 40) {
      await result.evaluate(element => { element.textContent = ''; });
      await page.mouse.move(box.x + box.width / cols * 22, box.y + box.height / 16);
      await expect.poll(() => result.getAttribute('data-hovered')).toBeNull();
      await page.mouse.click(box.x + box.width / cols * 22, box.y + box.height / 16);
      expect(await result.textContent()).toBe('');
    }
  } finally { await page.close(); }
}, 20_000);
