import { test, expect } from '@playwright/test';
import path from 'node:path';
import { launch, scratch, snapshot } from '../fixtures/electron';

test('Shellfox env editor, native menu actions, and Settings without obsolete CLI controls', async () => {
  test.skip(process.platform !== 'win32', 'Windows embedded-shell UI coverage.');
  const cwd = await scratch('shellfox-ui');
  const { app, page } = await launch(path.join(cwd, 'data'), 'real');
  try {
    // Test the bridge without overwriting the user's clipboard or opening Explorer.
    await app.evaluate(({ clipboard, shell, dialog }, cwd) => {
      const root = globalThis as any;
      root.__shellfoxRound3Originals = { writeText: clipboard.writeText, openPath: shell.openPath, showOpenDialog: dialog.showOpenDialog };
      clipboard.writeText = async text => { root.__shellfoxRound3Copied = text; };
      shell.openPath = async folder => { root.__shellfoxRound3Folder = folder; return ''; };
      dialog.showOpenDialog = async () => ({ canceled: false, filePaths: [cwd] });
    }, cwd);
    await page.getByRole('button', { name: 'New session', exact: true }).click();
    await page.getByRole('button', { name: 'Browse folders', exact: true }).click();
    const row = page.getByRole('button', { name: `Select session ${path.basename(cwd)}`, exact: true });
    await expect(row).toBeVisible();
    const before = (await snapshot(page)).sessions[0];
    await row.click({ button: 'right' });
    const menu = page.getByRole('menu');
    await expect(menu.getByRole('menuitem')).toHaveText(['Pin to top', 'Rename', 'Environment variables…', 'Open in File Explorer', 'Copy path', 'Settle', 'Delete…']);
    await menu.getByRole('menuitem', { name: 'Environment variables…', exact: true }).click();
    const modal = page.getByRole('dialog', { name: `Environment · ${path.basename(cwd)}`, exact: true });
    const editor = modal.getByRole('textbox', { name: 'Environment variables', exact: true });
    await expect(editor).toBeFocused();
    await editor.fill('PATH=one\npath=two');
    await expect(modal.getByText('Line 2: Duplicate variable name.', { exact: true })).toBeVisible();
    await expect(modal.getByRole('button', { name: 'Save', exact: true })).toBeDisabled();
    await editor.fill('# Local\nexport NODE_ENV="development"\nSHELLFOX_TEST_VALUE=with=equals');
    await editor.press('Control+Enter');
    await expect(modal).toBeHidden();
    await expect.poll(async () => (await snapshot(page)).sessions[0].env).toEqual([{ name: 'NODE_ENV', value: 'development' }, { name: 'SHELLFOX_TEST_VALUE', value: 'with=equals' }]);
    expect((await snapshot(page)).sessions[0].tabs.map(tab => [tab.id, tab.generation])).toEqual(before.tabs.map(tab => [tab.id, tab.generation]));
    await row.click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Environment variables…', exact: true }).click();
    await expect(editor).toHaveValue('NODE_ENV=development\nSHELLFOX_TEST_VALUE=with=equals');
    await editor.fill('UNSAVED=value'); await editor.press('Escape');
    await expect(modal).toBeHidden();
    expect((await snapshot(page)).sessions[0].env).toHaveLength(2);

    await row.click({ button: 'right' }); await page.getByRole('menuitem', { name: 'Copy path', exact: true }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Path copied' })).toBeVisible();
    expect(await app.evaluate(() => (globalThis as any).__shellfoxRound3Copied)).toBe(cwd);
    await expect(page.getByText('Path copied', { exact: true })).toBeHidden({ timeout: 4000 });
    await row.click({ button: 'right' }); await page.getByRole('menuitem', { name: 'Open in File Explorer', exact: true }).click();
    await expect.poll(() => app.evaluate(() => (globalThis as any).__shellfoxRound3Folder)).toBe(cwd);
    await app.evaluate(({ shell }) => { shell.openPath = async () => 'Fixture folder unavailable'; });
    await row.click({ button: 'right' }); await page.getByRole('menuitem', { name: 'Open in File Explorer', exact: true }).click();
    await expect(page.getByRole('alert')).toContainText('Fixture folder unavailable');

    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    const settings = page.getByRole('region', { name: 'Settings', exact: true });
    await expect(settings).toBeVisible();
    await expect(settings.getByRole('tablist', { name: 'Settings categories' }).getByRole('tab')).toHaveText(['General', 'Terminals', 'Agents', 'Remote access', 'SSH']);
    await settings.getByRole('tab', { name: 'Terminals', exact: true }).click();
    await expect(settings.getByRole('heading', { name: 'Default shell', exact: true })).toBeVisible();
    await expect(settings.getByRole('combobox', { name: 'Default shell', exact: true })).toBeVisible();
    // Integration is automatic. Do not restore install switches or obsolete root-command guidance.
    await expect(settings.getByRole('heading', { name: 'Terminal command', exact: true, includeHidden: true })).toHaveCount(0);
    await expect(settings.getByRole('switch', { name: 'Enable shellfox command in terminals', exact: true, includeHidden: true })).toHaveCount(0);
    await expect(settings.getByText('shellfox start .', { exact: true })).toHaveCount(0);
    expect((await snapshot(page)).cli.command).toBe('shellfox');
  } finally {
    // Dispose only this isolated test application's embedded shells.
    await page.evaluate(async () => {
      const result = await window.shellfox.getSnapshot();
      if (result.ok) for (const session of result.value.sessions) for (const tab of session.tabs) {
        if (tab.terminalKind === 'embedded' && tab.lifecycle !== 'closed' && tab.generation) await window.shellfox.closeTab!({ tabId: tab.id, generation: tab.generation });
      }
    }).catch(() => {});
    await app.evaluate(({ clipboard, shell, dialog }) => {
      const original = (globalThis as any).__shellfoxRound3Originals;
      if (original) { clipboard.writeText = original.writeText; shell.openPath = original.openPath; dialog.showOpenDialog = original.showOpenDialog; }
    });
    await app.close();
  }
});
