import { test, expect, type ElectronApplication, type Page } from '@playwright/test';
import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { launch, scratch } from '../fixtures/electron';
import { populate } from '../fixtures/files';

async function close(app: ElectronApplication) {
  await app.evaluate(({ BrowserWindow }) => {
    for (const window of BrowserWindow.getAllWindows()) window.webContents.on('will-prevent-unload', event => event.preventDefault());
  }).catch(() => {});
  await app.close();
}
async function navigate(page: Page, directory: string) {
  const input = page.getByRole('combobox', { name: 'Folder path', exact: true });
  await input.fill(directory + path.sep);
  // Escape dismisses completion without closing the sidebar; Enter opens the entered path.
  await expect(page.getByRole('listbox', { name: 'Matching folders' })).toBeVisible();
  await input.press('Escape');
  await expect(page.getByRole('listbox', { name: 'Matching folders' })).toHaveCount(0);
  await input.press('Enter');
}

test('folder icon and shared path picker work with zero sessions, collapse keeps file open', async () => {
  const directory = await scratch('files-global-ui'); await populate(directory);
  const { app, page } = await launch(path.join(directory, 'data'));
  try {
    const snapshot = await page.evaluate(() => window.shellfox.getSnapshot());
    expect(snapshot.ok && snapshot.value.sessions).toEqual([]);
    const toggle = page.getByRole('button', { name: 'Files', exact: true });
    await expect(toggle).toBeEnabled();
    expect((await toggle.innerText()).trim()).toBe('');
    await expect(toggle.locator('svg')).toHaveCount(1);
    await toggle.click();
    const sidebar = page.getByRole('complementary', { name: 'File sidebar', exact: true });
    await expect(sidebar).toBeVisible();
    await navigate(page, directory);
    await page.getByRole('button', { name: 'notes.txt', exact: true }).click();
    await expect(page.getByRole('textbox', { name: 'File contents' })).toHaveValue(/first line/);
    await app.evaluate(({ clipboard }) => {
      const copied: string[] = []; Reflect.set(globalThis, '__copiedFilePaths', copied);
      clipboard.writeText = async text => { copied.push(text); };
    });
    await page.getByRole('button', { name: 'Copy file path', exact: true }).click();
    await expect.poll(() => app.evaluate(() => Reflect.get(globalThis, '__copiedFilePaths'))).toEqual([path.join(directory, 'notes.txt')]);
    const collapse = sidebar.getByRole('button', { name: 'Toggle file browser', exact: true });
    expect((await collapse.innerText()).trim()).toBe('');
    await expect(collapse).toHaveAttribute('aria-expanded', 'true');
    await collapse.click();
    await expect(page.getByRole('tree', { name: 'Files', exact: true })).toHaveCount(0);
    await expect(page.getByRole('textbox', { name: 'File contents' })).toBeVisible();
    await expect(collapse).toHaveAttribute('aria-expanded', 'false');
    await collapse.click();
    await expect(page.getByRole('tree', { name: 'Files', exact: true })).toBeVisible();
    await toggle.click(); await expect(sidebar).toBeHidden();
    await toggle.click(); await expect(sidebar).toBeVisible();
    await expect(page.getByRole('textbox', { name: 'File contents' })).toHaveValue(/first line/);
  } finally { await close(app); }
});

test('session path picker dismisses suggestions before Escape closes and submits the entered folder', async () => {
  const directory = await scratch('cwd-picker-escape'); await populate(directory);
  const { app, page } = await launch(path.join(directory, 'data'));
  try {
    await page.getByRole('button', { name: 'New session', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'New session', exact: true });
    const input = dialog.getByRole('combobox', { name: 'Folder path', exact: true });
    await input.fill(directory + path.sep);
    await expect(dialog.getByRole('listbox', { name: 'Matching folders' })).toBeVisible();
    await input.press('Escape');
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole('listbox')).toHaveCount(0);
    await input.press('Enter');
    await expect(dialog).toHaveCount(0);
    await expect.poll(async () => {
      const snapshot = await page.evaluate(() => window.shellfox.getSnapshot());
      return snapshot.ok ? snapshot.value.sessions.map(session => session.cwd) : [];
    }).toContain(directory);
    await page.getByRole('button', { name: 'New session', exact: true }).click();
    await input.fill(directory + path.sep);
    await expect(dialog.getByRole('listbox')).toBeVisible();
    await input.press('Escape'); await expect(dialog).toBeVisible();
    await input.press('Escape'); await expect(dialog).toHaveCount(0);
  } finally { await close(app); }
});

test('wrapping long source lines changes layout without changing or losing edited text', async () => {
  const directory = await scratch('files-wrap-ui'); await populate(directory);
  const original = `const message = '${'long source '.repeat(70)}';\n`;
  await writeFile(path.join(directory, 'long.js'), original);
  const { app, page } = await launch(path.join(directory, 'data'));
  try {
    await page.getByRole('button', { name: 'Files', exact: true }).click();
    await navigate(page, directory);
    await page.getByRole('button', { name: 'long.js', exact: true }).click();
    const source = page.getByRole('textbox', { name: 'File contents', exact: true });
    await expect(source).toHaveValue(original);
    const before = await source.inputValue();
    const toggle = page.getByRole('button', { name: /Wrap|Unwrap/ });
    await toggle.click();
    const wrapA = await source.getAttribute('wrap');
    await toggle.click();
    const wrapB = await source.getAttribute('wrap');
    expect(wrapA).not.toBe(wrapB);
    await expect(source).toHaveValue(before);
    const changed = before + '// retained draft\n';
    await source.fill(changed);
    await toggle.click();
    await expect(source).toHaveValue(changed);
    await page.getByRole('button', { name: 'Save file', exact: true }).click();
    await expect.poll(() => readFile(path.join(directory, 'long.js'), 'utf8')).toBe(changed);
  } finally { await close(app); }
});
