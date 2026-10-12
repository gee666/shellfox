import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { launch, scratch, value } from '../fixtures/electron';

test('clicking a terminal URL opens it in the system browser', async () => {
  test.skip(process.platform === 'win32', 'POSIX shell command');
  test.setTimeout(120000);
  const dir = await scratch('links'), { app, page } = await launch(path.join(dir, 'data'), 'real');
  try {
    await app.evaluate(({ shell }) => {
      const opened: string[] = (globalThis as any).__opened = [];
      shell.openExternal = async url => { opened.push(url); };
    });
    const session = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID(), title: 'links' }));
    const tab = session.tabs[0]!;
    await page.getByRole('button', { name: 'Select session links', exact: true }).click();
    await expect(page.locator('.xterm-helper-textarea')).toBeEditable();
    value(await page.evaluate(input => window.shellfox.writeTerminal!(input), { tabId: tab.id, generation: tab.generation!, data: "clear; printf 'https://example.com/%s\\n' shellfox-link\r" }));
    await expect(page.locator('.xterm-rows')).toContainText('https://example.com/shellfox-link', { timeout: 15000 });
    const row = page.locator('.xterm-rows > div', { hasText: 'https://example.com/shellfox-link' }).last();
    const box = (await row.boundingBox())!;
    await page.mouse.move(box.x + 40, box.y + box.height / 2);
    await page.mouse.move(box.x + 50, box.y + box.height / 2);
    await page.mouse.click(box.x + 50, box.y + box.height / 2);
    await expect.poll(() => app.evaluate(() => (globalThis as any).__opened), { timeout: 10000 }).toEqual(['https://example.com/shellfox-link']);
  } finally {
    await page.evaluate(async () => {
      const result = await window.shellfox.getSnapshot();
      if (result.ok) for (const s of result.value.sessions) for (const t of s.tabs) if (t.terminalKind === 'embedded' && t.lifecycle !== 'closed') await window.shellfox.closeTab!({ tabId: t.id, generation: t.generation! });
    }).catch(() => {});
    await app.close();
  }
});

test('the eye button toggles keep awake', async () => {
  const dir = await scratch('awake'), { app, page } = await launch(path.join(dir, 'data'));
  try {
    await app.evaluate(({ powerSaveBlocker }) => {
      const calls: string[] = (globalThis as any).__power = [];
      powerSaveBlocker.start = type => { calls.push('start:' + type); return 99; };
      powerSaveBlocker.stop = id => { calls.push('stop:' + id); return true; };
    });
    const eye = page.getByRole('button', { name: 'Keep awake' });
    await expect(eye).toHaveAttribute('aria-pressed', 'false');
    await eye.click(); await expect(eye).toHaveAttribute('aria-pressed', 'true');
    await eye.click(); await expect(eye).toHaveAttribute('aria-pressed', 'false');
    expect(await app.evaluate(() => (globalThis as any).__power)).toEqual(['start:prevent-display-sleep', 'stop:99']);
    await eye.hover();
    expect(await eye.evaluate(node => getComputedStyle(node, '::after').content)).toBe('"Keep awake"');
  } finally { await app.close(); }
});
