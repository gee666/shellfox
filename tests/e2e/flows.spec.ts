import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { mkdir } from 'node:fs/promises';
import { launch, scratch, snapshot, observe, calls, value } from '../fixtures/electron';

test('UI keeps same-folder sessions and their tabs in expanded Local through settle, unsettle and confirmed deletion', async () => {
  const dir = await scratch('flows');
  const cwd = path.join(dir, "same folder Ω ' & ; % [data]");
  await mkdir(cwd);
  const { app, page } = await launch(path.join(dir, 'data'));
  try {
    await app.evaluate(({ dialog }, cwd) => { dialog.showOpenDialog = async () => ({ canceled: false, filePaths: [cwd] }); }, cwd);
    for (const title of ['First session', 'Second session']) {
      await page.getByRole('button', { name: 'New session', exact: true }).click();
      await page.getByRole('button', { name: 'Browse folders', exact: true }).click();
      const selected = page.getByRole('button', { name: `Select session ${path.basename(cwd)}`, exact: true }).last();
      await expect(selected).toHaveAttribute('aria-pressed', 'true');
      await selected.click({ button: 'right' });
      await page.getByRole('menuitem', { name: 'Rename', exact: true }).click();
      await page.getByLabel('Session title', { exact: true }).fill(title);
      await page.getByLabel('Session title', { exact: true }).press('Enter');
      await expect(page.getByRole('button', { name: `Select session ${title}`, exact: true })).toBeVisible();
    }
    await expect.poll(async () => (await snapshot(page)).sessions.length).toBe(2);
    const sessions = (await snapshot(page)).sessions;
    expect(new Set(sessions.map(s => s.id)).size).toBe(2);
    expect(sessions.map(s => s.cwd)).toEqual([cwd, cwd]);
    expect(sessions.every(s => s.canFocus && s.tabs[0].lifecycle === 'open')).toBe(true);
    await observe(app, 1);
    await page.getByRole('button', { name: 'New terminal', exact: true }).click();
    await expect(page.getByRole('tab')).toHaveCount(2);
    await expect.poll(async () => (await calls(app)).watch).toBe(3);
    await observe(app, 1);
    const before = await calls(app);
    // Retired external-terminal UI is gone; retain direct legacy focus API coverage.
    await expect(page.getByRole('button', { name: 'Focus', exact: true })).toHaveCount(0);
    value(await page.evaluate(id => window.shellfox.focusSession({ sessionId: id }), sessions[0].id));
    await page.getByRole('tab').first().click();
    value(await page.evaluate(id => window.shellfox.focusSession({ sessionId: id }), sessions[0].id));
    expect((await calls(app)).launches).toBe(before.launches);
    expect((await calls(app)).focuses).toBe(before.focuses + 2);
    const second = page.getByRole('button', { name: 'Select session Second session', exact: true });
    const local = page.getByRole('button', { name: 'Local · 2', exact: true });
    await expect(local).toHaveAttribute('aria-expanded', 'true');
    const original = (await snapshot(page)).sessions.find(s => s.title === 'Second session')!;
    await second.click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Settle', exact: true }).click();
    await expect(page.getByRole('dialog')).toHaveCount(0);
    await expect.poll(async () => (await snapshot(page)).sessions.find(s => s.id === original.id)?.status).toBe('settled');
    const settled = (await snapshot(page)).sessions.find(s => s.id === original.id)!;
    expect(settled.settledAt).not.toBeNull();
    expect(settled.tabs).toEqual(original.tabs);
    expect(settled.counts.agents).toBe(original.counts.agents);
    expect((await calls(app)).watch).toBe(3);
    await expect(second).toBeVisible();
    await expect(page.getByRole('tab')).toHaveCount(2);
    await local.click();
    await expect(local).toHaveAttribute('aria-expanded', 'false');
    await expect(second).toHaveCount(0);
    expect((await snapshot(page)).sessions).toHaveLength(2);
    await local.click();
    await expect(second).toBeVisible();
    await second.click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Unsettle', exact: true }).click();
    await expect.poll(async () => (await snapshot(page)).sessions.find(s => s.id === original.id)?.settledAt).toBeNull();
    const restored = (await snapshot(page)).sessions.find(s => s.id === original.id)!;
    expect(restored.status).not.toBe('settled');
    expect(restored.tabs).toEqual(original.tabs);
    expect((await calls(app)).launches).toBe(3);

    // Live sessions can be deleted directly, but cancellation must keep their tabs.
    await second.click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Delete…', exact: true }).click();
    const deletion = page.getByRole('dialog', { name: 'Delete session' });
    await deletion.getByRole('button', { name: 'Cancel', exact: true }).click();
    expect((await snapshot(page)).sessions.find(s => s.id === original.id)?.tabs).toEqual(original.tabs);
    expect((await calls(app)).watch).toBe(3);
    await second.click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Delete…', exact: true }).click();
    await deletion.getByRole('button', { name: 'Delete', exact: true }).click();
    await expect(page.getByRole('button', { name: 'Local · 1', exact: true })).toBeVisible();
    await expect(second).toHaveCount(0);
    expect((await snapshot(page)).sessions.map(s => s.title)).toEqual(['First session']);
    expect((await calls(app)).watch).toBe(1);
    await expect(page.getByRole('button', { name: /Archived/ })).toHaveCount(0);
  } finally { await app.close(); }
});

test('sticky focus failure, inaccessible observations and broker loss never relaunch', async () => {
  const dir = await scratch('errors');
  const { app, page } = await launch(path.join(dir, 'data'));
  try {
    const session = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID() }));
    await expect.poll(async () => (await calls(app)).watch).toBe(1);
    await observe(app, 0);
    expect((await snapshot(page)).sessions[0].status).toBe('waiting');
    await app.evaluate(() => { (globalThis as any).__shellfoxTest.backend.focusDenied = true; });
    const result = await page.evaluate(id => window.shellfox.focusSession({ sessionId: id }), session.id);
    expect(result.ok ? null : result.error.code).toBe('FOCUS_DENIED');
    await observe(app, 0);
    expect((await snapshot(page)).sessions[0].status).toBe('error');
    value(await page.evaluate(id => window.shellfox.clearSessionError({ sessionId: id }), session.id));
    await observe(app, 0, 'unavailable', 'unknown');
    expect((await snapshot(page)).sessions[0].status).toBe('unknown');
    await app.evaluate(() => (globalThis as any).__shellfoxTest.backend.emit({ type: 'unavailable', error: { code: 'NATIVE_UNAVAILABLE', message: 'Fixture broker crash', retryable: true } }));
    const after = (await snapshot(page)).sessions[0];
    expect(after.canFocus).toBe(false);
    expect(after.canAddTab).toBe(false);
    expect(after.status).toBe('unknown');
    expect((await calls(app)).launches).toBe(1);
  } finally { await app.close(); }
});

test('uncertain launch requires confirmation, ignores stale registration and supports delayed registration', async () => {
  const dir = await scratch('registration');
  const { app, page } = await launch(path.join(dir, 'data'));
  try {
    await app.evaluate(() => { (globalThis as any).__shellfoxTest.backend.mode = 'timeout'; });
    const session = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID() }));
    expect(session.tabs[0].lifecycle).toBe('launch-uncertain');
    expect(session.status).toBe('unknown');
    const retry = await page.evaluate(tabId => window.shellfox.retryTab({ tabId, confirmPossibleDuplicate: false }), session.tabs[0].id);
    expect(retry.ok ? null : retry.error.code).toBe('RETRY_CONFIRM_REQUIRED');
    expect((await calls(app)).launches).toBe(1);
    await app.evaluate(() => { (globalThis as any).__shellfoxTest.backend.mode = 'register-after-receipt'; });
    value(await page.evaluate(tabId => window.shellfox.retryTab({ tabId, confirmPossibleDuplicate: true }), session.tabs[0].id));
    await expect.poll(async () => (await snapshot(page)).sessions[0].tabs[0].lifecycle).toBe('open');
    await expect.poll(async () => (await calls(app)).watch).toBe(1);
    // Inspector evaluation can interrupt a synchronous SQLite query. Schedule
    // database work on the normal event loop instead of re-entering the connection.
    const registeredPid = await app.evaluate(() => new Promise<number>((resolve, reject) => setImmediate(() => {
      try {
        const fixture = (globalThis as any).__shellfoxTest;
        const old = fixture.backend.launches[0];
        fixture.backend.emit({ type: 'registered', registration: { sessionId: old.sessionId, tabId: old.tabId, operationId: old.operationId, shell: { pid: 9999, startTime: '134000000000000001' }, shellExecutable: old.shellExecutable, cwd: old.cwd, registeredAt: new Date().toISOString() }, target: null });
        resolve(fixture.repository.tabs()[0].registration.shell.pid);
      } catch (error) { reject(error); }
    })));
    expect(registeredPid).not.toBe(9999);
    await observe(app, 0, 'exited');
    expect((await snapshot(page)).sessions[0].status).toBe('unknown');
    expect((await snapshot(page)).sessions[0].canFocus).toBe(false);
    expect((await calls(app)).launches).toBe(2);
  } finally { await app.close(); }
});
