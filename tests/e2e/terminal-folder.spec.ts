import { expect, test, type ElectronApplication } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { launch, scratch, seedTerminal, value } from '../fixtures/electron';
import { populate } from '../fixtures/files';

async function changeDirectory(app: ElectronApplication, tabId: string, generation: string, directory: string) {
  const quote = (text: string) => `'${text.replaceAll("'", process.platform === 'win32' ? "''" : "'\\''")}'`;
  const command = process.platform === 'win32'
    ? `Set-Location -LiteralPath ${quote(directory)}; Write-Output 'SHELLFOX_CWD_READY'\r`
    : `cd -- ${quote(directory)} && printf 'SHELLFOX_CWD_READY\\n'\r`;
  value(await seedTerminal(app, { tabId, generation, data: command }));
}

test('separate terminal-folder action follows actual cwd after cd and preserves a dirty preview', async () => {
  const directory = await scratch('terminal-current-folder'); await populate(directory);
  const { app, page } = await launch(path.join(directory, 'data'), 'real');
  try {
    const session = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: directory, title: 'CWD files', requestId: randomUUID() }));
    await page.getByRole('button', { name: 'Select session CWD files', exact: true }).click();
    const tab = session.tabs[0];
    if (!tab?.generation) throw new Error('No live terminal for current-folder test.');
    await page.getByRole('button', { name: 'Files', exact: true }).click();
    await page.getByRole('button', { name: 'notes.txt', exact: true }).click();
    const source = page.getByRole('textbox', { name: 'File contents', exact: true });
    await source.fill('preserve this unsaved preview\n');
    const picker = page.getByRole('combobox', { name: 'Folder path', exact: true });
    await picker.fill(path.join(directory, 'assets'));
    await page.getByRole('button', { name: 'Go', exact: true }).click();
    await expect(page.getByRole('button', { name: 'app.js', exact: true })).toBeVisible();
    await page.getByRole('button', { name: 'Toggle file browser', exact: true }).click();
    await page.getByRole('button', { name: 'Files', exact: true }).click();

    const current = path.join(directory, 'nested folder');
    await changeDirectory(app, tab.id, tab.generation, current);
    await expect.poll(async () => {
      const result = await page.evaluate(input => window.shellfox.files?.list(input), { sessionId: session.id, tabId: tab.id, path: '.' });
      return result?.ok ? result.value.path : null;
    }).toBe(current);
    expect(session.cwd).not.toBe(current);
    const action = page.getByRole('button', { name: 'Open terminal folder', exact: true });
    expect((await action.innerText()).trim()).toBe(''); await expect(action.locator('svg')).toHaveCount(1);
    await action.click();
    await expect(picker).toHaveValue(current);
    await expect(page.getByRole('button', { name: 'child.txt', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Toggle file browser', exact: true })).toHaveAttribute('aria-expanded', 'true');
    await expect(source).toHaveValue('preserve this unsaved preview\n');
    await expect(page.getByRole('alertdialog', { name: 'Unsaved changes' })).toHaveCount(0);
    // Explicitly discard the fixture draft before closing Electron, not through a beforeunload dialog.
    await page.getByRole('button', { name: 'Close file sidebar', exact: true }).click();
    await page.getByRole('alertdialog', { name: 'Unsaved changes', exact: true }).getByRole('button', { name: 'Discard changes', exact: true }).click();
  } finally {
    await page.evaluate(async () => {
      const snapshot = await window.shellfox.getSnapshot();
      if (snapshot.ok) for (const session of snapshot.value.sessions) for (const tab of session.tabs) {
        if (tab.generation && tab.lifecycle !== 'closed') await window.shellfox.closeTab?.({ tabId: tab.id, generation: tab.generation });
      }
    }).catch(() => {});
    await app.evaluate(({ BrowserWindow }) => {
      for (const window of BrowserWindow.getAllWindows()) window.webContents.on('will-prevent-unload', event => event.preventDefault());
    }).catch(() => {});
    await app.close();
  }
});
