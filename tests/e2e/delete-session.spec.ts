import { test, expect, type Page } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import type { OwnedTerminal } from '../../src/main/terminal/backend';
import { launch, scratch, snapshot, value } from '../fixtures/electron';

const select = (page: Page, title: string) => page.getByRole('button', { name: `Select session ${title}`, exact: true });

for (const settled of [false, true]) test(`UI confirms deletion of a ${settled ? 'settled' : 'live'} session, closes its PTYs and keeps it deleted after restart`, async ({}, testInfo) => {
  test.setTimeout(120000);
  const dir = await scratch('delete-session'), data = path.join(dir, 'data');
  let running = await launch(data, 'real');
  const live = (): Promise<OwnedTerminal[]> => running.app.evaluate(() => (globalThis as any).__shellfoxTest.backend.live());
  const identities = async (id: string) => (await live()).filter(t => t.sessionId === id).map(t => [t.tabId, t.generation, t.root.pid]);
  try {
    let page = running.page;
    const keep = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID(), title: 'Keep me' }));
    const doomed = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID(), title: 'Delete me' }));
    const withTabs = value(await page.evaluate(input => window.shellfox.addTab(input), { sessionId: doomed.id }));
    await expect.poll(async () => (await live()).filter(t => t.sessionId === doomed.id).length).toBe(2);
    const keepRoots = await identities(keep.id), doomedRoots = await identities(doomed.id);
    await select(page, 'Delete me').click();
    await expect(page.getByLabel('Interactive terminal')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Local · 2', exact: true })).toHaveAttribute('aria-expanded', 'true');
    if (settled) {
      await page.getByRole('button', { name: 'Settle session Delete me', exact: true }).click();
      await expect(page.getByRole('dialog')).toHaveCount(0);
      await expect.poll(async () => (await snapshot(page)).sessions.find(s => s.id === doomed.id)?.status).toBe('settled');
      expect((await snapshot(page)).sessions.find(s => s.id === doomed.id)?.tabs.map(t => t.id)).toEqual(withTabs.tabs.map(t => t.id));
      expect(await identities(doomed.id)).toEqual(doomedRoots);
      await expect(page.getByLabel('Interactive terminal')).toBeVisible();
    }

    await select(page, 'Delete me').click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Delete…', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'Delete session' });
    await expect(dialog).toContainText('Delete me');
    await expect(dialog).toContainText('Its running terminals will be closed.');
    await expect(dialog).toContainText('This cannot be undone.');
    await testInfo.attach('delete-confirmation', { body: await page.screenshot(), contentType: 'image/png' });
    expect((await snapshot(page)).sessions).toHaveLength(2);
    expect(await identities(doomed.id)).toEqual(doomedRoots);
    await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(dialog).toHaveCount(0);
    expect((await snapshot(page)).sessions.find(s => s.id === doomed.id)?.tabs.map(t => t.id)).toEqual(withTabs.tabs.map(t => t.id));
    expect(await identities(doomed.id)).toEqual(doomedRoots);
    expect(await identities(keep.id)).toEqual(keepRoots);

    await select(page, 'Delete me').click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Delete…', exact: true }).click();
    await dialog.getByRole('button', { name: 'Delete', exact: true }).click();
    await expect(page.getByRole('button', { name: 'Local · 1', exact: true })).toBeVisible();
    await expect(select(page, 'Delete me')).toHaveCount(0);
    await expect.poll(async () => (await live()).some(t => t.sessionId === doomed.id)).toBe(false);
    expect(await identities(keep.id)).toEqual(keepRoots);
    await expect(select(page, 'Keep me')).toBeVisible();
    expect((await snapshot(page)).sessions.map(s => s.id)).toEqual([keep.id]);
    for (const tab of withTabs.tabs) {
      const attachment = await page.evaluate(id => window.shellfox.attachTerminal!({ tabId: id }), tab.id);
      expect(attachment.ok).toBe(false);
    }
    const gone = await page.evaluate(id => window.shellfox.deleteSession({ sessionId: id }), doomed.id);
    expect(gone.ok ? null : gone.error.code).toBe('NOT_FOUND');

    await running.app.close();
    running = await launch(data, 'real'); page = running.page;
    expect((await snapshot(page)).sessions.some(s => s.id === doomed.id)).toBe(false);
    const rows = await running.app.evaluate((_electron, id) => new Promise<{ session: unknown; tabs: unknown[] }>((resolve, reject) => setImmediate(() => {
      try {
        const repository = (globalThis as any).__shellfoxTest.repository;
        resolve({ session: repository.session(id) ?? null, tabs: repository.tabs(id) });
      } catch (error) { reject(error); }
    })), doomed.id);
    expect(rows.session).toBeNull(); expect(rows.tabs).toEqual([]);
    await expect(page.getByRole('button', { name: 'Local · 1', exact: true })).toBeVisible();
    await testInfo.attach('after-restart', { body: await page.screenshot(), contentType: 'image/png' });
  } finally {
    // Only close terminals owned by this isolated test application.
    await running.page.evaluate(async () => {
      const result = await window.shellfox.getSnapshot();
      if (result.ok) for (const session of result.value.sessions) for (const tab of session.tabs) {
        if (tab.terminalKind === 'embedded' && tab.lifecycle !== 'closed' && tab.generation) await window.shellfox.closeTab!({ tabId: tab.id, generation: tab.generation });
      }
    }).catch(() => {});
    await running.app.close();
  }
});
