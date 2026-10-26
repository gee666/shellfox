import { test, expect } from '@playwright/test';
import path from 'node:path';
import { mkdir } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { launch, scratch, snapshot, value } from '../fixtures/electron';

test('session path prompt completes real folders, resolves home paths and retains the native picker', async () => {
  const dir = await scratch('directory-prompt');
  const project = path.join(dir, 'my project');
  await mkdir(path.join(project, 'nested'), { recursive: true });
  const { app, page } = await launch(path.join(dir, 'data'));
  try {
    await page.getByRole('button', { name: 'New session', exact: true }).click();
    const input = page.getByRole('textbox', { name: 'Folder path' });
    await expect(input).toBeFocused();
    const home = value(await page.evaluate(() => window.shellfox.getHomeDirectory())).cwd;
    await expect(page.getByLabel(`Working directory: ${home}`, { exact: true })).toBeVisible();
    await page.screenshot({ path: 'tmp/screenshots/new-session-home.png' });
    const homeFolders = value(await page.evaluate(() => window.shellfox.completeDirectory({ path: '' }))).matches;
    const example = homeFolders.find(folder => /^documents[\\/]$/i.test(folder)) ?? homeFolders[0];
    if (example) {
      await input.fill(example.slice(0, -1));
      await input.press('Tab');
      await expect(input).toHaveValue(example);
      await page.screenshot({ path: 'tmp/screenshots/new-session-completed.png' });
    }
    await input.fill(path.join(dir, 'my'));
    await input.press('Tab');
    await expect(input).toHaveValue(project + path.sep);
    await expect(input).toBeFocused();
    await input.press('Tab');
    await expect(input).toHaveValue(path.join(project, 'nested') + path.sep);
    await input.press('Enter');
    await expect(page.getByRole('dialog', { name: 'New session', exact: true })).toBeHidden();
    expect((await snapshot(page)).sessions[0].cwd).toBe(path.join(project, 'nested'));
    await page.getByRole('button', { name: 'New session', exact: true }).click();
    const missing = path.join(dir, 'missing-parent', 'child');
    await input.fill(missing);
    await input.press('Enter');
    await expect(page.getByRole('alertdialog', { name: 'Create missing folder?' })).toBeVisible();
    expect(existsSync(missing)).toBe(false);
    await page.getByRole('button', { name: 'Keep editing', exact: true }).click();
    await expect(input).toBeFocused(); await expect(input).toHaveValue(missing); expect(existsSync(missing)).toBe(false);
    await input.press('Enter'); await page.getByRole('button', { name: 'Create folder', exact: true }).click();
    await expect(page.getByRole('dialog', { name: 'New session', exact: true })).toBeHidden();
    expect(existsSync(missing)).toBe(true); expect((await snapshot(page)).sessions.some(session => session.cwd === missing)).toBe(true);
    await page.getByRole('button', { name: 'New session', exact: true }).click();
    await input.fill('');
    await input.press('Enter');
    await expect(page.getByRole('dialog', { name: 'New session', exact: true })).toBeHidden();
    expect((await snapshot(page)).sessions.some(session => session.cwd === home)).toBe(true);
    await app.evaluate(({ dialog }, cwd) => { dialog.showOpenDialog = async () => ({ canceled: false, filePaths: [cwd] }); }, project);
    await page.getByRole('button', { name: 'New session', exact: true }).click();
    await input.press('Shift+Tab');
    await expect(page.getByRole('button', { name: 'Browse folders' })).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(page.getByRole('dialog', { name: 'New session', exact: true })).toBeHidden();
    expect((await snapshot(page)).sessions.some(session => session.cwd === project)).toBe(true);
  } finally { await app.close(); }
});
