import { test, expect } from '@playwright/test';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { launch, scratch, snapshot, observe, calls, value } from '../fixtures/electron';

test('Local pagination, settled tabs, settings and identities survive manager restart without commands', async () => {
  const dir = await scratch('restart');
  const data = path.join(dir, 'data');
  const requestId = randomUUID();
  let { app, page } = await launch(data);
  let closed = false;
  try {
    expect((await snapshot(page)).settings.historyPageSize).toBe(10);
    const original = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, title: 'literal %_ Ω', requestId }));
    const duplicate = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId }));
    expect(duplicate.id).toBe(original.id);
    for (let n = 0; n < 11; n++) {
      const created = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, title: `Local ${String(n).padStart(2, '0')}`, requestId: randomUUID() }));
      value(await page.evaluate(sessionId => window.shellfox.settleSession({ sessionId, confirmActive: false }), created.id));
    }
    await expect.poll(async () => (await calls(app)).watch).toBe(12);
    await observe(app, 1);
    const before = (await snapshot(page)).sessions;
    const ids = before.map(s => s.id).sort();
    expect(before.filter(s => s.status === 'settled')).toHaveLength(11);
    expect(before.every(s => s.tabs.length === 1 && s.tabs[0].lifecycle === 'open')).toBe(true);
    const local = page.getByRole('region', { name: 'Local sessions', exact: true });
    const rows = local.getByRole('button', { name: /^Select session / });
    await expect(page.getByRole('button', { name: 'Local · 12', exact: true })).toHaveAttribute('aria-expanded', 'true');
    await expect(rows).toHaveCount(10);
    await page.getByRole('button', { name: 'Show more', exact: true }).click();
    await expect(rows).toHaveCount(12);
    await expect(page.getByRole('button', { name: 'Show more', exact: true })).toHaveCount(0);
    const titles = await rows.allTextContents();
    await observe(app, 1);
    await expect(rows).toHaveText(titles);

    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    const settings = page.getByRole('region', { name: 'Settings', exact: true });
    await settings.getByLabel('Sessions per page', { exact: true }).fill('3');
    await expect.poll(async () => (await snapshot(page)).settings.historyPageSize).toBe(3);
    await page.getByRole('button', { name: 'Use #2dd4bf', exact: true }).click();
    await expect.poll(async () => (await snapshot(page)).settings.accentColor).toBe('#2dd4bf');
    await settings.getByRole('button', { name: 'Close settings', exact: true }).click();
    await expect(settings).toBeHidden();
    await expect(rows).toHaveCount(3);
    await page.getByRole('button', { name: 'Show more', exact: true }).click();
    await expect(rows).toHaveCount(6);
    expect((await snapshot(page)).sessions).toHaveLength(12);
    const identities = await app.evaluate(() => (globalThis as any).__shellfoxTest.backend.watch.map((t: any) => t.registration));
    const pragmas = await app.evaluate(() => new Promise<unknown[]>((resolve, reject) => setImmediate(() => {
      try {
        const db = (globalThis as any).__shellfoxTest.repository.db;
        resolve([db.pragma('user_version', { simple: true }), db.pragma('foreign_keys', { simple: true }), db.pragma('journal_mode', { simple: true })]);
      } catch (error) { reject(error); }
    })));
    expect(pragmas).toEqual([5, 1, 'wal']);
    await app.close(); closed = true;
    ({ app, page } = await launch(data)); closed = false;
    const restored = await snapshot(page);
    expect(restored.settings).toMatchObject({ accentColor: '#2dd4bf', historyPageSize: 3 });
    expect(restored.sessions.map(s => s.id).sort()).toEqual(ids);
    expect(restored.sessions.filter(s => s.status === 'settled')).toHaveLength(11);
    expect(restored.sessions.every(s => s.tabs.length === 1)).toBe(true);
    for (const session of restored.sessions) {
      expect(session.tabs.map(t => [t.id, t.generation])).toEqual(before.find(s => s.id === session.id)!.tabs.map(t => [t.id, t.generation]));
    }
    expect((await calls(app)).launches).toBe(0);
    expect(await app.evaluate(() => (globalThis as any).__shellfoxTest.backend.watch.map((t: any) => t.registration))).toEqual(identities);
    await expect(page.getByRole('button', { name: 'Local · 12', exact: true })).toHaveAttribute('aria-expanded', 'true');
    await expect(page.getByRole('region', { name: 'Local sessions' }).getByRole('button', { name: /^Select session / })).toHaveCount(3);
    await observe(app, 1);
    expect((await snapshot(page)).sessions.filter(s => s.status === 'settled')).toHaveLength(11);
    const settled = restored.sessions.find(s => s.settledAt)!;
    value(await page.evaluate(sessionId => window.shellfox.unsettleSession({ sessionId }), settled.id));
    const unmarked = (await snapshot(page)).sessions.find(s => s.id === settled.id)!;
    expect(unmarked.settledAt).toBeNull();
    expect(unmarked.status).toBe('running');
    expect(unmarked.tabs.map(t => t.id)).toEqual(settled.tabs.map(t => t.id));
    expect((await snapshot(page)).sessions.map(s => s.id).sort()).toEqual(ids);
    expect((await calls(app)).launches).toBe(0);
  } finally { if (!closed) await app.close(); }
});

test('restart converts incomplete persisted launch to uncertain without relaunch', async () => {
  const dir = await scratch('incomplete');
  const data = path.join(dir, 'data');
  let { app, page } = await launch(data);
  let closed = false;
  try {
    await app.evaluate(() => { (globalThis as any).__shellfoxTest.backend.mode = 'pending'; });
    const session = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId: randomUUID() }));
    expect(session.tabs[0].lifecycle).toBe('launching');
    await app.close(); closed = true;
    ({ app, page } = await launch(data)); closed = false;
    const restored = (await snapshot(page)).sessions[0];
    expect(restored.id).toBe(session.id);
    expect(restored.tabs[0].lifecycle).toBe('launch-uncertain');
    expect(restored.status).toBe('unknown');
    expect(restored.canFocus).toBe(false);
    expect((await calls(app)).launches).toBe(0);
  } finally { if (!closed) await app.close(); }
});
