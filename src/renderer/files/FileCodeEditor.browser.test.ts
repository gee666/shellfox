import { chromium } from '@playwright/test';
import { build } from 'esbuild';
import { readFile } from 'node:fs/promises';
import { expect, it } from 'vitest';

it('keeps textarea and syntax highlight wrapping aligned after edits, resizing and horizontal scrolling', async () => {
  const text = Array.from({ length: 35 }, (_, index) => `const row${index} = "${'long-word '.repeat(25)}";`).join('\n');
  const compiled = await build({ stdin: { loader: 'tsx', resolveDir: process.cwd(), contents: `
    import React, {useState} from 'react'; import {createRoot} from 'react-dom/client';
    import {FileCodeEditor} from './src/renderer/files/FileCodeEditor';
    function App(){const [text,setText]=useState(${JSON.stringify(text)});return <FileCodeEditor path="file.ts" text={text} request={null} onChange={setText} onSave={()=>{}}/>}
    createRoot(document.getElementById('root')).render(<App/>);
  ` }, bundle: true, write: false, format: 'iife', define: { 'process.env.NODE_ENV': '"production"' } });
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage();
    await page.setContent('<style>body{margin:0;--fg:#ddd;--panel:#17171b;--border:#555}#root{display:flex;width:320px;height:300px}</style><div id="root"></div>');
    await page.addStyleTag({ content: await readFile('src/renderer/files/files.css', 'utf8') });
    await page.addScriptTag({ content: compiled.outputFiles[0].text });
    const input = page.getByRole('textbox', { name: 'File contents' });
    const metrics = () => page.locator('.file-code-editor').evaluate(element => {
      const input = element.querySelector('textarea'), pre = element.querySelector('pre');
      if (!input || !pre) throw new Error('Missing source layers');
      return { inputWidth: input.clientWidth, preWidth: pre.clientWidth, inputHeight: input.scrollHeight, preHeight: pre.scrollHeight,
        inputScroll: input.scrollTop, preScroll: pre.scrollTop, inputLeft: input.scrollLeft, preLeft: pre.scrollLeft,
        whitespace: getComputedStyle(input).whiteSpace, preWhitespace: getComputedStyle(pre).whiteSpace,
        wrap: getComputedStyle(input).overflowWrap, preWrap: getComputedStyle(pre).overflowWrap };
    });
    await input.waitFor();
    await expect.poll(async () => { const value = await metrics(); return [value.preWidth - value.inputWidth, value.preHeight - value.inputHeight]; }).toEqual([0, 0]);
    expect(await metrics()).toMatchObject({ whitespace: 'pre-wrap', preWhitespace: 'pre-wrap', wrap: 'anywhere', preWrap: 'anywhere' });
    await page.locator('#root').evaluate(element => { element.style.width = '221px'; });
    await expect.poll(async () => { const value = await metrics(); return [value.preWidth - value.inputWidth, value.preHeight - value.inputHeight]; }).toEqual([0, 0]);
    await input.fill(text + '\n// unsaved ' + 'x'.repeat(500));
    await page.getByRole('button', { name: 'Unwrap', exact: true }).click();
    expect(await input.inputValue()).toBe(text + '\n// unsaved ' + 'x'.repeat(500));
    await input.evaluate(element => { element.scrollTop = 100; element.scrollLeft = 80; element.dispatchEvent(new Event('scroll')); });
    expect(await metrics()).toMatchObject({ inputScroll: 100, preScroll: 100, inputLeft: 80, preLeft: 80, whitespace: 'pre', preWhitespace: 'pre' });
    await page.getByRole('button', { name: 'Wrap', exact: true }).click();
    await expect.poll(async () => { const value = await metrics(); return value.preHeight - value.inputHeight; }).toBe(0);
    // clientWidth rounds226.5 to227, enough to fit one extra character per line.
    await input.fill('x'.repeat(1000));
    await page.locator('#root').evaluate(element => { element.style.width = '226.5px'; });
    await expect.poll(async () => { const value = await metrics(); return value.preHeight - value.inputHeight; }).toBe(0);
    expect(await page.locator('pre').evaluate(element => element.getBoundingClientRect().width)).toBe(226.5);
  } finally { await browser.close(); }
}, 20_000);
