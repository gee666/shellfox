import { test, expect, type ElectronApplication, type Locator, type Page } from '@playwright/test';
import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { launch, scratch } from '../fixtures/electron';

async function close(app: ElectronApplication) {
  await app.evaluate(({ BrowserWindow }) => {
    for (const window of BrowserWindow.getAllWindows()) window.webContents.on('will-prevent-unload', event => event.preventDefault());
  }).catch(() => {});
  await app.close();
}
async function openFolder(page: Page, directory: string) {
  await page.getByRole('button', { name: 'Files', exact: true }).click();
  const input = page.getByRole('combobox', { name: 'Folder path', exact: true });
  await input.fill(directory); await input.press('Escape'); await input.press('Enter');
}
async function drag(page: Page, handle: Locator, dx: number, dy = 0) {
  const box = await handle.boundingBox(); if (!box) throw new Error('Resize handle not visible');
  const x = box.x + box.width / 2, y = box.y + box.height / 2;
  await page.mouse.move(x, y); await page.mouse.down();
  await page.mouse.move(x + dx, y + dy, { steps: 8 }); await page.mouse.up();
  await expect(page.locator('.layout-resize-shield')).toHaveCount(0);
}
async function size(locator: Locator, dimension: 'width' | 'height') {
  const box = await locator.boundingBox(); if (!box) throw new Error('Panel not visible'); return box[dimension];
}

test('both sidebar handles and the tree split resize without replacing a dirty file; maximize fills only app content', async () => {
  const directory = await scratch('files-layout'); await writeFile(path.join(directory, 'draft.txt'), 'original');
  const { app, page } = await launch(path.join(directory, 'data'));
  try {
    await openFolder(page, directory); await page.getByRole('button', { name: 'draft.txt', exact: true }).click();
    const editor = page.getByRole('textbox', { name: 'File contents', exact: true }); await editor.fill('retained dirty draft');
    const originalEditor = await editor.elementHandle();
    const sidebar = page.getByRole('complementary', { name: 'File sidebar', exact: true });
    const browser = page.getByRole('region', { name: 'File browser', exact: true });
    const width = page.getByRole('separator', { name: 'File sidebar width', exact: true });
    const split = page.getByRole('separator', { name: 'File browser height', exact: true });
    const initialWidth = await size(sidebar, 'width');
    await drag(page, width, -80); await expect.poll(() => size(sidebar, 'width')).toBeCloseTo(initialWidth + 80, 0);
    const initialHeight = await size(browser, 'height');
    await drag(page, split, 0, 50); await expect.poll(() => size(browser, 'height')).toBeCloseTo(initialHeight + 50, 0);
    await drag(page, page.getByRole('separator', { name: 'Sidebar width', exact: true }), 32);
    const restoredWidth = await size(sidebar, 'width'), restoredHeight = await size(browser, 'height');
    const maximize = page.getByRole('button', { name: 'Maximize file', exact: true });
    await expect(maximize.locator('svg')).toHaveCount(1); expect((await maximize.innerText()).trim()).toBe('');
    await maximize.click(); await expect(sidebar).toHaveAttribute('data-maximized', 'true');
    const bounds = await sidebar.boundingBox(), viewport = await page.evaluate(() => ({ width: innerWidth, height: innerHeight }));
    expect(bounds?.x).toBe(0); expect(bounds?.y).toBe(0);
    expect(bounds?.width).toBe(viewport.width); expect(bounds?.height).toBe(viewport.height);
    expect(await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().some(window => window.isFullScreen()))).toBe(false);
    await expect(page.getByRole('complementary', { name: 'Session navigation' })).toBeHidden();
    await expect(width).toBeHidden(); await expect(editor).toHaveValue('retained dirty draft');
    expect(await editor.evaluate((node, original) => node === original, originalEditor)).toBe(true);
    await editor.fill('edited while maximized'); await page.getByRole('button', { name: 'Save file', exact: true }).click();
    await expect.poll(() => readFile(path.join(directory, 'draft.txt'), 'utf8')).toBe('edited while maximized');
    await page.getByRole('button', { name: 'Restore file', exact: true }).click();
    await expect.poll(() => size(sidebar, 'width')).toBeCloseTo(restoredWidth, 0);
    await expect.poll(() => size(browser, 'height')).toBeCloseTo(restoredHeight, 0);
    expect(await editor.evaluate((node, original) => node === original, originalEditor)).toBe(true);
    await expect(editor).toHaveValue('edited while maximized');
    await width.focus(); await width.press('Home'); await expect(width).toHaveAttribute('aria-valuenow', '420');
    await split.dblclick(); await expect(split).toHaveAttribute('aria-valuenow', '220');
  } finally { await close(app); }
});

test('interactive HTML input, script counter and iframe identity survive resizing and maximize/restore', async () => {
  const directory = await scratch('files-layout-html');
  await writeFile(path.join(directory, 'interactive.html'), '<!doctype html><label>Message<input aria-label="Message" value="original"></label><button onclick="this.textContent = `Count ${Number(this.dataset.count = Number(this.dataset.count || 0) + 1)}`">Count 0</button>');
  const { app, page } = await launch(path.join(directory, 'data'));
  try {
    await openFolder(page, directory); await page.getByRole('button', { name: 'interactive.html', exact: true }).click();
    const frameElement = page.getByTitle('HTML preview', { exact: true }), originalFrame = await frameElement.elementHandle();
    const frame = page.frameLocator('iframe[title="HTML preview"]');
    await frame.getByRole('textbox', { name: 'Message' }).fill('interactive value');
    await frame.getByRole('button', { name: 'Count 0', exact: true }).press('Enter');
    await expect(frame.getByRole('button', { name: 'Count 1', exact: true })).toBeVisible();
    await drag(page, page.getByRole('separator', { name: 'File sidebar width', exact: true }), -60);
    await drag(page, page.getByRole('separator', { name: 'File browser height', exact: true }), 0, 40);
    await page.getByRole('button', { name: 'Maximize file', exact: true }).click();
    await expect(frame.getByRole('textbox', { name: 'Message' })).toHaveValue('interactive value');
    await frame.getByRole('button', { name: 'Count 1', exact: true }).press('Enter');
    await expect(frame.getByRole('button', { name: 'Count 2', exact: true })).toBeVisible();
    await page.getByRole('button', { name: 'Toggle file browser', exact: true }).click();
    await page.getByRole('button', { name: 'Restore file', exact: true }).click();
    expect(await frameElement.evaluate((node, original) => node === original, originalFrame)).toBe(true);
    await expect(frame.getByRole('textbox', { name: 'Message' })).toHaveValue('interactive value');
    await frame.getByRole('button', { name: 'Count 2', exact: true }).press('Enter');
    await expect(frame.getByRole('button', { name: 'Count 3', exact: true })).toBeVisible();
  } finally { await close(app); }
});

test('narrow workspaces fill available space and restore desktop sizing without an extra maximize button', async () => {
  const directory = await scratch('files-layout-narrow'); await writeFile(path.join(directory, 'note.txt'), 'small window');
  const { app, page } = await launch(path.join(directory, 'data'));
  try {
    await openFolder(page, directory); await page.getByRole('button', { name: 'note.txt', exact: true }).click();
    const sidebar = page.getByRole('complementary', { name: 'File sidebar', exact: true });
    const width = page.getByRole('separator', { name: 'File sidebar width', exact: true });
    await width.focus(); await width.press('ArrowLeft'); const preferred = await width.getAttribute('aria-valuenow');
    await app.evaluate(({ BrowserWindow }) => { const window = BrowserWindow.getAllWindows()[0]!; window.setMinimumSize(400, 300); window.setContentSize(640, 480); });
    await expect(sidebar).toHaveAttribute('data-narrow', 'true');
    await expect(page.getByRole('button', { name: 'Maximize file', exact: true })).toBeHidden(); await expect(width).toBeHidden();
    expect(await size(sidebar, 'width')).toBe(await size(page.locator('.workspace-content'), 'width'));
    await expect(page.getByRole('textbox', { name: 'File contents', exact: true })).toHaveValue('small window');
    await app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.setContentSize(1220, 820));
    await expect(sidebar).toHaveAttribute('data-narrow', 'false'); await expect(width).toHaveAttribute('aria-valuenow', preferred!);
    await expect(page.getByRole('button', { name: 'Maximize file', exact: true })).toBeVisible();
  } finally { await close(app); }
});
