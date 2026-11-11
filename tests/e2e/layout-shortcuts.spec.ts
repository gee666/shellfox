import { randomUUID } from 'node:crypto';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { expect, test, type ElectronApplication } from '@playwright/test';
import type { PtyBackend } from '../../src/main/terminal/backend';
import { launch, scratch, value } from '../fixtures/electron';

type RecordedInput = typeof globalThis & { __shellfoxTest: { backend: PtyBackend }; __layoutInput: string[] };
const modifier = process.platform === 'darwin' ? 'Meta' : 'Control';

async function close(app: ElectronApplication) {
  for (const page of app.windows()) page.on('dialog', dialog => { void dialog.accept().catch(() => {}); });
  await app.evaluate(({ BrowserWindow }) => {
    for (const window of BrowserWindow.getAllWindows()) window.webContents.on('will-prevent-unload', event => event.preventDefault());
  }).catch(() => {});
  await app.close();
}

test('sessions collapse to a small rail, preserve terminal identity and restore across reload', async () => {
  const directory = await scratch('sessions-sidebar');
  const { app, page } = await launch(path.join(directory, 'data'), 'real');
  try {
    const session = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: directory, requestId: randomUUID(), title: 'Layout test' }));
    const tab = session.tabs[0]!;
    await page.getByRole('button', { name: 'Select session Layout test', exact: true }).click();
    const input = page.locator('.xterm-helper-textarea'); await expect(input).toBeEditable();
    const original = await input.elementHandle();
    await app.evaluate((_electron, tabId) => {
      const state = globalThis as RecordedInput;
      const write = state.__shellfoxTest.backend.write.bind(state.__shellfoxTest.backend);
      state.__layoutInput = [];
      state.__shellfoxTest.backend.write = payload => {
        const result = write(payload);
        if (result.ok && payload.tabId === tabId) state.__layoutInput.push(payload.data);
        return result;
      };
    }, tab.id);
    const handle = page.getByRole('separator', { name: 'Sidebar width', exact: true });
    await handle.focus(); await handle.press('ArrowRight');
    const preferred = await handle.getAttribute('aria-valuenow');
    await input.focus(); await page.keyboard.press(`${modifier}+b`);
    await expect(page.getByRole('button', { name: 'Show sessions', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Select session Layout test', exact: true })).toBeHidden();
    await expect(handle).toHaveCount(0);
    expect((await page.locator('.sidebar').boundingBox())?.width).toBe(28);
    expect(await input.evaluate((node, previous) => node === previous, original)).toBe(true);
    await expect(input).toBeFocused();
    await page.keyboard.press(`${modifier}+b`);
    await expect(handle).toHaveAttribute('aria-valuenow', preferred!);
    expect(await input.evaluate((node, previous) => node === previous, original)).toBe(true);
    // Focus reporting is enabled by some shells. Only those notifications may reach the PTY.
    expect(await app.evaluate(() => (globalThis as RecordedInput).__layoutInput.join('').replace(/\x1b\[[IO]/g, ''))).toBe('');
    await page.getByRole('button', { name: 'Hide sessions', exact: true }).click();
    await page.reload(); await expect(page.getByRole('button', { name: 'Show sessions', exact: true })).toBeVisible();
    await page.getByRole('button', { name: 'Show sessions', exact: true }).click();
    await expect(handle).toHaveAttribute('aria-valuenow', preferred!);
  } finally { await close(app); }
});

test('filesystem and folders shortcuts retain the editor and unsaved draft', async () => {
  const directory = await scratch('files-shortcuts'); await writeFile(path.join(directory, 'note.txt'), 'original');
  const { app, page } = await launch(path.join(directory, 'data'));
  try {
    await page.keyboard.press(`${modifier}+Shift+b`);
    const folder = page.getByRole('combobox', { name: 'Folder path', exact: true });
    await expect(folder).not.toHaveValue('~');
    await folder.fill(directory); await expect(folder).toHaveValue(directory);
    await folder.press('Escape'); await folder.press('Enter');
    await page.getByRole('button', { name: 'note.txt', exact: true }).click();
    const editor = page.getByRole('textbox', { name: 'File contents', exact: true }); await editor.fill('unsaved draft');
    const original = await editor.elementHandle();
    await page.keyboard.press(`${modifier}+Shift+e`);
    await expect(page.getByRole('button', { name: 'Toggle file browser', exact: true })).toHaveAttribute('aria-expanded', 'false');
    await page.keyboard.press(`${modifier}+Shift+b`);
    await expect(page.getByRole('complementary', { name: 'File sidebar', exact: true })).toBeHidden();
    await expect(page.getByRole('button', { name: 'Files', exact: true })).toBeFocused();
    await page.keyboard.press(`${modifier}+Shift+b`);
    await expect(editor).toHaveValue('unsaved draft');
    expect(await editor.evaluate((node, previous) => node === previous, original)).toBe(true);
    await page.keyboard.press(`${modifier}+Shift+e`); await expect(folder).toBeVisible();
    await expect(editor).toHaveValue('unsaved draft');
    await page.getByRole('button', { name: 'Save file', exact: true }).click();
    await expect(page.getByRole('button', { name: 'Save file', exact: true })).toBeDisabled();
  } finally { await close(app); }
});
