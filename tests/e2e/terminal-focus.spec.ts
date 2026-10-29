import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { launch, scratch, value } from '../fixtures/electron';

test('new sessions and tabs focus terminal input without a terminal click', async () => {
  const dir = await scratch('terminal-focus');
  const { app, page } = await launch(path.join(dir, 'data'), 'real');
  try {
    value(await page.evaluate(input => window.shellfox.createSession(input), {
      cwd: dir, requestId: randomUUID(), title: 'Terminal focus',
    }));
    await page.getByRole('button', { name: 'Select session Terminal focus', exact: true }).click();
    await expect(page.locator('.xterm-helper-textarea')).toBeFocused();
    await page.keyboard.type('echo first');
    await expect(page.locator('.xterm-rows')).toContainText('echo first');
    await page.getByRole('button', { name: 'New terminal', exact: true }).click();
    await expect(page.getByRole('tab')).toHaveCount(2);
    await expect(page.locator('.xterm-helper-textarea')).toBeFocused();
    await page.keyboard.type('echo second');
    await expect(page.locator('.xterm-rows')).toContainText('echo second');
  } finally {
    await page.evaluate(async () => {
      const result = await window.shellfox.getSnapshot();
      if (result.ok) for (const session of result.value.sessions) for (const tab of session.tabs) {
        if (tab.terminalKind === 'embedded' && tab.lifecycle !== 'closed') {
          await window.shellfox.closeTab!({ tabId: tab.id, generation: tab.generation! });
        }
      }
    }).catch(() => {});
    await app.close();
  }
});
