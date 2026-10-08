import { test, expect } from '@playwright/test';
import { launch, scratch } from '../fixtures/electron';

test('has no application menu on any platform', async () => {
  const { app, page } = await launch(await scratch('application-menu'));
  try {
    expect(await app.evaluate(({ Menu }) => Menu.getApplicationMenu() === null)).toBe(true);
    // macOS uses the global application menu rather than a window menu bar.
    if (process.platform !== 'darwin') {
      const visible = () => app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows().some(window => window.isMenuBarVisible()));
      expect(await visible()).toBe(false);
      await page.keyboard.press('Alt');
      expect(await visible()).toBe(false);
    }
  } finally { await app.close(); }
});
