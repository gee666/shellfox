import { test, expect, type Page } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { mkdir } from 'node:fs/promises';
import { launch, scratch, snapshot, value } from '../fixtures/electron';

// Real embedded backend (real PTY shells, real SQLite), no fake native adapter: runs on Linux and macOS too.
const shots = path.resolve('tmp/e2e-evidence');
const history = (page: Page) => page.evaluate(() => window.shellfox.getHistory({ search: '', status: 'all', page: 1, pageSize: 20 })).then(value);
const archivedRegion = (page: Page) => page.getByRole('region', { name: 'Archived sessions' });
const select = (page: Page, title: string) => page.getByRole('button', { name: `Select session ${title}`, exact: true });

test('an archived session is deleted permanently through the UI, only after confirmation, and stays gone after restart', async () => {
  test.skip(process.platform === 'win32', 'Uses POSIX-shell timing; the Windows fake-backend variant lives in flows.spec.ts.');
  test.setTimeout(120000);
  await mkdir(shots, { recursive: true });
  const dir = await scratch('delete-archived'), data = path.join(dir, 'data');
  let running = await launch(data, 'real');
  try {
    let page = running.page;
    const keep = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID(), title: 'Keep me' }));
    const doomed = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID(), title: 'Delete me' }));
    await select(page, 'Delete me').click();
    await expect(page.getByLabel('Interactive terminal')).toBeVisible();
    await expect.poll(async () => (await snapshot(page)).sessions.find(s => s.id === doomed.id)!.tabs[0]!.status, { timeout: 20000 }).not.toBe('opening');
    await page.screenshot({ path: path.join(shots, 'delete-1-two-live-sessions.png') });

    // Live sessions have no delete entry, and the API refuses them.
    await select(page, 'Delete me').click({ button: 'right' });
    await expect(page.getByRole('menuitem', { name: 'Archive', exact: true })).toBeVisible();
    await expect(page.getByRole('menuitem', { name: 'Delete permanently…', exact: true })).toHaveCount(0);
    await page.keyboard.press('Escape');
    const early = await page.evaluate(id => window.shellfox.deleteSession({ sessionId: id }), doomed.id);
    expect(early.ok ? null : early.error.code).toBe('UNSUPPORTED');

    // Archive through the UI (a running shell needs the extra confirmation when the app asks for it).
    await select(page, 'Delete me').click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Archive', exact: true }).click();
    const archiveConfirm = page.getByRole('dialog', { name: 'Archive session' });
    if (await archiveConfirm.waitFor({ timeout: 3000 }).then(() => true, () => false)) await archiveConfirm.getByRole('button', { name: 'Archive', exact: true }).click();
    await expect(page.getByRole('button', { name: 'Archived · 1', exact: true })).toBeVisible();
    await page.getByRole('button', { name: 'Archived · 1', exact: true }).click();
    await expect(archivedRegion(page)).toContainText('Delete me');
    await page.screenshot({ path: path.join(shots, 'delete-2-archived-listed.png') });

    // Archiving never stops the shell, so the confirmation says deleting will close it. Cancel keeps everything.
    const shell = (await history(page)).items.find(s => s.id === doomed.id)!.tabs[0]!;
    expect(shell.lifecycle).toBe('open');
    await archivedRegion(page).getByRole('button', { name: 'Select session Delete me', exact: true }).click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Delete permanently…', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'Delete session' });
    await expect(dialog).toContainText('Permanently delete “Delete me”? Its running terminals will be closed. This cannot be undone.');
    await page.screenshot({ path: path.join(shots, 'delete-3-confirm-dialog.png') });
    await archivedRegion(page).getByRole('button', { name: 'Select session Delete me', exact: true }).click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Delete permanently…', exact: true }).click();
    await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(dialog).toHaveCount(0);
    expect((await history(page)).total).toBe(1);
    expect(await running.app.evaluate((_electron, id) => (globalThis as any).__shellfoxTest.backend.get(id)?.state, shell.id)).toBe('open');
    await archivedRegion(page).getByRole('button', { name: 'Select session Delete me', exact: true }).click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Delete permanently…', exact: true }).click();
    await dialog.getByRole('button', { name: 'Delete', exact: true }).click();
    await expect(page.getByRole('button', { name: 'Archived · 0', exact: true })).toBeVisible();
    await expect(select(page, 'Delete me')).toHaveCount(0);
    // The shell of the deleted session was closed, not orphaned.
    await expect.poll(() => running.app.evaluate((_electron, id) => (globalThis as any).__shellfoxTest.backend.get(id)?.state ?? 'gone', shell.id)).not.toBe('open');
    await expect(select(page, 'Keep me')).toBeVisible();
    await page.screenshot({ path: path.join(shots, 'delete-5-after-delete.png') });
    expect((await history(page)).total).toBe(0);
    expect((await snapshot(page)).sessions.map(s => s.id)).toEqual([keep.id]);
    const gone = await page.evaluate(id => window.shellfox.deleteSession({ sessionId: id }), doomed.id);
    expect(gone.ok ? null : gone.error.code).toBe('NOT_FOUND');

    // Straight from the database after a restart.
    await page.evaluate(async () => { const result = await window.shellfox.getSnapshot(); if (result.ok) for (const s of result.value.sessions) for (const t of s.tabs) if (t.terminalKind === 'embedded' && t.lifecycle !== 'closed') await window.shellfox.closeTab!({ tabId: t.id, generation: t.generation! }); });
    await running.app.close();
    running = await launch(data, 'real'); page = running.page;
    expect((await history(page)).total).toBe(0);
    expect((await snapshot(page)).sessions.some(s => s.id === doomed.id)).toBe(false);
    const rows = await running.app.evaluate((_electron, id) => {
      const repository = (globalThis as any).__shellfoxTest.repository;
      return { session: repository.session(id) ?? null, tabs: repository.tabs(id), all: repository.sessions().map((s: { title: string }) => s.title) };
    }, doomed.id);
    expect(rows.session).toBeNull(); expect(rows.tabs).toEqual([]);
    await page.screenshot({ path: path.join(shots, 'delete-6-after-restart.png') });
  } finally {
    await running.page.evaluate(async () => { const result = await window.shellfox.getSnapshot(); if (result.ok) for (const s of result.value.sessions) for (const t of s.tabs) if (t.terminalKind === 'embedded' && t.lifecycle !== 'closed') await window.shellfox.closeTab!({ tabId: t.id, generation: t.generation! }); }).catch(() => {});
    await running.app.close();
  }
});
