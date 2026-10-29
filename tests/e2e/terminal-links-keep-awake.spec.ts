import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { seedTerminal, launch, scratch, value } from '../fixtures/electron';

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
    value(await seedTerminal(app, { tabId: tab.id, generation: tab.generation!, data: "clear; printf 'https://example.com/%s\\n' shellfox-link\r" }));
    await expect(page.locator('.xterm-rows')).toContainText('https://example.com/shellfox-link', { timeout: 15000 });
    const row = page.locator('.xterm-rows > div', { hasText: 'https://example.com/shellfox-link' }).last();
    const box = (await row.boundingBox())!;
    await page.mouse.move(box.x + 40, box.y + box.height / 2);
    await page.mouse.move(box.x + 50, box.y + box.height / 2);
    await page.mouse.click(box.x + 50, box.y + box.height / 2);
    await expect.poll(() => app.evaluate(() => (globalThis as any).__opened), { timeout: 10000 }).toEqual(['https://example.com/shellfox-link']);
    // A TUI with mouse reporting receives the click and opens the link itself.
    value(await seedTerminal(app, { tabId: tab.id, generation: tab.generation!, data: "printf '\\033[?1000h\\033[?1006h'\r" }));
    await page.waitForTimeout(500);
    await page.mouse.move(box.x + 40, box.y + box.height / 2);
    await page.mouse.move(box.x + 50, box.y + box.height / 2);
    await page.mouse.click(box.x + 50, box.y + box.height / 2);
    await page.waitForTimeout(500);
    expect(await app.evaluate(() => (globalThis as any).__opened)).toHaveLength(1);
    await page.keyboard.down('Shift');
    await page.mouse.click(box.x + 50, box.y + box.height / 2);
    await page.keyboard.up('Shift');
    await expect.poll(() => app.evaluate(() => (globalThis as any).__opened), { timeout: 10000 }).toHaveLength(2);
  } finally {
    await page.evaluate(async () => {
      const result = await window.shellfox.getSnapshot();
      if (result.ok) for (const s of result.value.sessions) for (const t of s.tabs) if (t.terminalKind === 'embedded' && t.lifecycle !== 'closed') await window.shellfox.closeTab!({ tabId: t.id, generation: t.generation! });
    }).catch(() => {});
    await app.close();
  }
});

test('URLs broken inside a table column and OSC 8 labels open from every row', async () => {
  test.skip(process.platform === 'win32', 'POSIX shell command');
  test.setTimeout(120000);
  const dir = await scratch('table-links'), { app, page } = await launch(path.join(dir, 'data'), 'real');
  try {
    await app.evaluate(({ shell }) => {
      const opened: string[] = (globalThis as any).__opened = [];
      shell.openExternal = async url => { opened.push(url); };
    });
    const session = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID(), title: 'table-links' }));
    const tab = session.tabs[0]!;
    await page.getByRole('button', { name: 'Select session table-links', exact: true }).click();
    await expect(page.locator('.xterm-helper-textarea')).toBeEditable();
    // Shape of pi's table fallback: "(url)" hard-broken at the column edge, then an OSC 8 label
    // followed by the hyperlink opt-ins the shell received.
    const command = "clear; printf '\\342\\224\\202 (https://example.com/col \\342\\224\\202\\n\\342\\224\\202 umn/end)                 \\342\\224\\202\\n'; "
      + "printf '\\033]8;;https://example.com/osc8\\033\\\\osc8label\\033]8;;\\033\\\\ env=%s%s\\n' \"$FORCE_HYPERLINK\" \"$PI_HYPERLINKS\"\r";
    value(await seedTerminal(app, { tabId: tab.id, generation: tab.generation!, data: command }));
    await expect(page.locator('.xterm-rows')).toContainText('osc8label env=11', { timeout: 15000 });
    const click = async (text: string, offset: number) => {
      const box = (await page.locator('.xterm-rows > div', { hasText: text }).last().boundingBox())!;
      await page.mouse.move(box.x + offset - 5, box.y + box.height / 2);
      await page.mouse.move(box.x + offset, box.y + box.height / 2);
      await page.mouse.click(box.x + offset, box.y + box.height / 2);
    };
    await click('umn/end)', 40);
    await expect.poll(() => app.evaluate(() => (globalThis as any).__opened), { timeout: 10000 }).toEqual(['https://example.com/column/end']);
    await click('osc8label', 20);
    await expect.poll(() => app.evaluate(() => (globalThis as any).__opened), { timeout: 10000 }).toEqual(['https://example.com/column/end', 'https://example.com/osc8']);
    await expect(page.locator('.terminal-surface[title="https://example.com/osc8"]')).toHaveCount(1);
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
