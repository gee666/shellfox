import { test, expect, _electron } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import { mkdir } from 'node:fs/promises';
import path from 'node:path';
import { env, root, scratch, snapshot, testMain, value } from '../fixtures/electron';

test('in-app quit confirmation defaults to Cancel, preserves owned shells on cancel and closes after approval', async ({}, testInfo) => {
  test.setTimeout(60000);
  const dir = await scratch('quit-confirmation');
  const app = await _electron.launch({
    cwd: root,
    env: { ...env(), SHELLFOX_TEST_VISIBLE: '0', SHELLFOX_TEST_CONFIRM_QUIT: '1' },
    args: [testMain, '--test-user-data', path.join(dir, 'data'), '--test-backend', 'real'],
    timeout: 30000,
  });
  let closed = false;
  app.on('close', () => { closed = true; });
  const nativeDialogs: string[] = [];
  app.process().stderr?.on('data', (data: Buffer) => {
    if (data.toString().includes('E2E_UNEXPECTED_NATIVE_DIALOG')) nativeDialogs.push(data.toString());
  });
  const page = await app.firstWindow();
  try {
    await page.waitForFunction(() => !!window.shellfox);
    await expect(page.getByRole('main', { name: 'Workspace' })).toBeVisible();
    const windowId = await app.evaluate(({ BrowserWindow }) => {
      const owned = BrowserWindow.getAllWindows()[0];
      if (!owned) throw new Error('The test window is missing.');
      owned.hide();
      owned.setPosition(-10000, -10000);
      return owned.id;
    });
    expect(await app.evaluate(({ BrowserWindow }, id) => BrowserWindow.fromId(id)?.isVisible(), windowId)).toBe(false);
    // A native-dialog regression must fail this test, never block the test desktop.
    await app.evaluate(({ dialog }) => {
      const unexpected = () => { console.error('E2E_UNEXPECTED_NATIVE_DIALOG'); };
      dialog.showMessageBoxSync = () => { unexpected(); return 0; };
      dialog.showMessageBox = async () => { unexpected(); return { response: 0, checkboxChecked: false }; };
      dialog.showErrorBox = () => { unexpected(); };
    });
    const session = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID(), title: 'Quit guard' }));
    const withTabs = value(await page.evaluate(input => window.shellfox.addTab(input), { sessionId: session.id }));
    expect(withTabs.tabs).toHaveLength(2);
    const original = withTabs.tabs.map(tab => [tab.id, tab.generation]);
    const subscriptions = await page.evaluate(async () => {
      const before = await window.shellfox.getCloseConfirmation!();
      if (!before.ok) throw new Error(before.error.message);
      const off = window.shellfox.subscribeCloseConfirmation!(() => {});
      off();
      return before.value;
    });
    expect(subscriptions).toBeNull();

    await app.evaluate(({ BrowserWindow }, id) => { BrowserWindow.fromId(id)?.close(); }, windowId);
    const dialog = page.getByRole('dialog', { name: 'Quit Shellfox?', exact: true });
    await expect(dialog).toBeVisible();
    expect(await app.evaluate(({ BrowserWindow }, id) => BrowserWindow.fromId(id)?.isVisible(), windowId)).toBe(false);
    await expect(dialog.getByRole('button', { name: 'Cancel', exact: true })).toBeFocused();
    const first = value(await page.evaluate(() => window.shellfox.getCloseConfirmation!()));
    expect(first).toMatchObject({ kind: 'quit', terminalCount: 2 });
    if (!first) throw new Error('Expected a pending quit confirmation.');
    const evidence = path.join(root, 'tmp/e2e-evidence');
    await mkdir(evidence, { recursive: true });
    await testInfo.attach('in-app-quit-confirmation', { body: await page.screenshot({ path: path.join(evidence, 'quit-confirmation.png') }), contentType: 'image/png' });
    await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(dialog).toHaveCount(0);
    expect(value(await page.evaluate(() => window.shellfox.getCloseConfirmation!()))).toBeNull();
    expect(closed).toBe(false);
    const retained = (await snapshot(page)).sessions.find(item => item.id === session.id)!;
    expect(retained.tabs.map(tab => [tab.id, tab.generation])).toEqual(original);
    for (const tab of retained.tabs) {
      const attachment = value(await page.evaluate(input => window.shellfox.attachTerminal!(input), { tabId: tab.id, generation: tab.generation! }));
      expect(attachment.control).toBe('owned');
      if (attachment.control !== 'owned') throw new Error('Local terminal unexpectedly controlled elsewhere.');
      expect(attachment.state).toBe('open');
    }

    await app.evaluate(({ app }) => { app.quit(); });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole('button', { name: 'Cancel', exact: true })).toBeFocused();
    const second = value(await page.evaluate(() => window.shellfox.getCloseConfirmation!()));
    expect(second).toMatchObject({ kind: 'quit', terminalCount: 2 });
    expect(second?.id).not.toBe(first?.id);
    const stale = await page.evaluate(id => window.shellfox.respondCloseConfirmation!({ id, approved: true }), first.id);
    expect(stale.ok ? null : stale.error.code).toBe('NOT_FOUND');
    await expect(dialog).toBeVisible();
    expect(nativeDialogs).toEqual([]);
    const didClose = app.waitForEvent('close', { timeout: 20000 });
    await dialog.getByRole('button', { name: 'Close shells and quit', exact: true }).click();
    await didClose;
    expect(closed).toBe(true);
    expect(nativeDialogs).toEqual([]);
  } finally {
    if (!closed) {
      // Failed assertions must not leave an opted-in confirmation waiting during teardown.
      await page.evaluate(async () => {
        window.shellfox.subscribeCloseConfirmation!(confirmation => {
          if (confirmation) void window.shellfox.respondCloseConfirmation!({ id: confirmation.id, approved: true });
        });
        const pending = await window.shellfox.getCloseConfirmation!();
        if (pending.ok && pending.value) await window.shellfox.respondCloseConfirmation!({ id: pending.value.id, approved: true });
      }).catch(() => {});
      await app.close();
    }
  }
});
