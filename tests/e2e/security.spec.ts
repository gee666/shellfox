import { test, expect } from '@playwright/test';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { writeFile } from 'node:fs/promises';
import { launch, scratch, snapshot, calls, value } from '../fixtures/electron';

test('sandbox, explicit preload, IPC sender/method validation, CSP and literal rendering', async () => {
  const dir = await scratch('security');
  const { app, page } = await launch(path.join(dir, 'data'));
  try {
    const isolation = await page.evaluate(() => ({ require: typeof (window as any).require, process: typeof (window as any).process, raw: typeof (window as any).ipcRenderer, test: typeof (window as any).__shellfoxTest, keys: Object.keys(window.shellfox).sort() }));
    expect(isolation.require).toBe('undefined'); expect(isolation.process).toBe('undefined');
    expect(isolation.raw).toBe('undefined'); expect(isolation.test).toBe('undefined');
    expect(isolation.keys).toEqual(['activateSession', 'prepareSessionRegistration', 'refreshSessionMembership', 'addTab', 'chooseDirectory', 'clearSessionError', 'createSession', 'focusSession', 'getHistory', 'getSnapshot', 'renameSession', 'retryTab', 'saveSettings', 'setExplorerIntegration', 'settleSession', 'subscribe', 'unsettleSession', 'getTerminalProfiles', 'attachTerminal', 'writeTerminal', 'resizeTerminal', 'closeTab', 'acknowledgeTerminal', 'detachTerminal', 'subscribeTerminal', 'copyText', 'openSessionFolder', 'setSessionEnv', 'setCliIntegration'].sort());
    const prefs = await app.evaluate(({ BrowserWindow }) => (BrowserWindow.getAllWindows()[0].webContents as any).getLastWebPreferences());
    expect(prefs.sandbox).toBe(true); expect(prefs.contextIsolation).toBe(true); expect(prefs.nodeIntegration).toBe(false);
    // Invoke Electron's installed handler from main to supply hostile sender frames.
    // No test-only channel or renderer raw IPC is added to the application.
    const codes = await app.evaluate(async ({ ipcMain, BrowserWindow }) => {
      const handler = (ipcMain as any)._invokeHandlers.get('manager:request');
      if (!handler) throw new Error('Electron IPC handler inspection unavailable');
      const wc = BrowserWindow.getAllWindows()[0].webContents;
      const trusted = { sender: wc, senderFrame: wc.mainFrame };
      const cases = [
        [{ sender: {}, senderFrame: wc.mainFrame }, { version: 1, method: 'getSnapshot', payload: {} }],
        [{ sender: wc, senderFrame: { url: wc.mainFrame.url } }, { version: 1, method: 'getSnapshot', payload: {} }],
        [trusted, { version: 2, method: 'getSnapshot', payload: {} }],
        [trusted, { version: 1, method: 'dispose', payload: {} }],
        [trusted, { version: 1, method: 'getSnapshot', payload: { arbitrary: true } }],
        [trusted, { version: 1, method: 'createSession', payload: { cwd: 'C:\\', requestId: 'bad' } }],
      ];
      return Promise.all(cases.map(async ([event, raw]) => { const r = await handler(event, raw); return r.ok ? 'unexpected success' : r.error.code; }));
    });
    expect(codes).toEqual(['AUTH_FAILED', 'AUTH_FAILED', 'VALIDATION', 'VALIDATION', 'VALIDATION', 'VALIDATION']);
    const title = '<img src=x onerror="window.pwned=1">';
    value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID(), title }));
    await expect(page.getByRole('complementary', { name: 'Session navigation' })).toContainText(title);
    expect(await page.locator('img').count()).toBe(0);
    expect(await page.evaluate(() => (window as any).pwned)).toBeUndefined();
    const popup = await page.evaluate(() => window.open('https://example.com'));
    expect(popup).toBeNull();
    expect(app.windows().length).toBe(1);
    const connectBlocked = await page.evaluate(async () => { try { await fetch('https://example.com'); return false; } catch { return true; } });
    expect(connectBlocked).toBe(true);
    const originalUrl = page.url();
    await page.evaluate(() => { location.href = 'https://example.com'; });
    await page.waitForTimeout(100);
    expect(page.url()).toBe(originalUrl);
    expect((await calls(app)).launches).toBe(1);
  } finally { await app.close(); }
});

test('invalid directories/settings reject before intent; preload subscriptions can be removed', async () => {
  const dir = await scratch('validation');
  const file = path.join(dir, 'file.txt'); await writeFile(file, 'not a folder');
  const { app, page } = await launch(path.join(dir, 'data'));
  try {
    for (const cwd of ['relative', '\\\\server\\share', 'shell:Downloads', file, path.join(dir, 'absent')]) {
      const result = await page.evaluate(input => window.shellfox.createSession(input), { cwd, requestId: randomUUID() });
      expect(result.ok ? null : result.error.code).toBe('VALIDATION');
    }
    expect((await snapshot(page)).sessions.length).toBe(0);
    expect((await calls(app)).launches).toBe(0);
    const invalid = await page.evaluate(async () => {
      const s = await window.shellfox.getSnapshot(); if (!s.ok) throw new Error('snapshot failed');
      return window.shellfox.saveSettings({ ...s.value.settings, accentColor: 'url(https://evil)' });
    });
    expect(invalid.ok ? null : invalid.error.code).toBe('VALIDATION');
    await page.evaluate(() => {
      const w = window as any; w.subscriptionCount = 0;
      const off = window.shellfox.subscribe(() => { w.subscriptionCount++; });
      off(); off();
    });
    value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID() }));
    await page.waitForTimeout(100);
    expect(await page.evaluate(() => (window as any).subscriptionCount)).toBe(0);
  } finally { await app.close(); }
});
