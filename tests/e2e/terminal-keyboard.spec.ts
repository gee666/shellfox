import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import type { PtyBackend } from '../../src/main/terminal/backend';
import { launch, scratch, value } from '../fixtures/electron';

type TestGlobal = typeof globalThis & {
  __shellfoxTest: { backend: PtyBackend };
  __keyboardInput: string[];
};

test('Shift letters reach the desktop PTY once without dropping intentional repeats or Unicode', async () => {
  const dir = await scratch('terminal-keyboard');
  const { app, page } = await launch(path.join(dir, 'data'), 'real');
  let identity: { tabId: string; generation: string } | undefined;
  try {
    const session = value(await page.evaluate(input => window.shellfox.createSession(input), {
      cwd: dir, requestId: randomUUID(), title: 'Keyboard regression',
    }));
    const tab = session.tabs[0]!;
    if (!tab.generation) throw new Error('Missing terminal generation');
    identity = { tabId: tab.id, generation: tab.generation };
    await page.getByRole('button', { name: 'Select session Keyboard regression', exact: true }).click();
    const textarea = page.locator('.xterm-helper-textarea');
    await expect(textarea).toBeEditable();
    await textarea.focus();
    await app.evaluate((_electron, tabId) => {
      const state = globalThis as TestGlobal;
      const backend = state.__shellfoxTest.backend, write = backend.write.bind(backend);
      state.__keyboardInput = [];
      backend.write = input => {
        const result = write(input);
        if (result.ok && input.tabId === tabId) state.__keyboardInput.push(input.data);
        return result;
      };
    }, tab.id);
    for (const letter of 'ABCDEFGHIJKLMNOPQRSTUVWXYZ') await page.keyboard.press(`Shift+${letter}`);
    await page.keyboard.type('AAZZaa!!');
    await page.keyboard.press('Shift+!');
    await page.keyboard.insertText('日本語🙂');
    await page.keyboard.press('Enter');
    await expect.poll(() => app.evaluate(() => (globalThis as TestGlobal).__keyboardInput.join('')))
      .toBe('ABCDEFGHIJKLMNOPQRSTUVWXYZAAZZaa!!!日本語🙂\r');
  } finally {
    if (identity) await page.evaluate(input => window.shellfox.closeTab!(input), identity).catch(() => {});
    await app.close();
  }
});
