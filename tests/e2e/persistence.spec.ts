import { test, expect } from '@playwright/test';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { launch, scratch, snapshot, observe, calls, value } from '../fixtures/electron';

test('SQLite history literal search, stable pagination, settings and identities survive manager restart without commands', async () => {
  const dir = await scratch('restart');
  const data = path.join(dir, 'data');
  const requestId = randomUUID();
  let { app, page } = await launch(data);
  let closed = false;
  try {
    const original = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, title: 'literal %_ Ω', requestId }));
    const duplicate = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId }));
    expect(duplicate.id).toBe(original.id);
    for (let n = 0; n < 6; n++) value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, title: `History ${n}`, requestId: randomUUID() }));
    await expect.poll(async () => (await calls(app)).watch).toBe(7);
    await observe(app, 1);
    const ids = (await snapshot(page)).sessions.map(s => s.id);
    for (const id of ids) value(await page.evaluate(sessionId => window.shellfox.settleSession({ sessionId, confirmActive: true }), id));
    // Deliberate timestamp ties prove ID tie-breaking with actual SQL and IPC pagination.
    await app.evaluate(() => (globalThis as any).__shellfoxTest.repository.db.prepare('UPDATE sessions SET settledAt=?').run('2026-01-01T00:00:00.000Z'));
    const pages = [];
    for (let pageNumber = 1; pageNumber <= 3; pageNumber++) pages.push(value(await page.evaluate(page => window.shellfox.getHistory({ search: '', status: 'running', page, pageSize: 3 }), pageNumber)));
    expect(pages.map(p => p.items.length)).toEqual([3, 3, 1]);
    expect(pages.every(p => p.total === 7)).toBe(true);
    expect(pages.flatMap(p => p.items.map(s => s.id))).toEqual([...ids].sort());
    for (const search of ['%', '%_']) {
      const result = value(await page.evaluate(search => window.shellfox.getHistory({ search, status: 'all', page: 1, pageSize: 20 }), search));
      expect(result.items.map(s => s.id)).toEqual([original.id]);
    }
    // Every fixture CWD contains Windows_Worshop. '_' therefore legitimately matches all.
    expect(value(await page.evaluate(() => window.shellfox.getHistory({ search: '_', status: 'all', page: 1, pageSize: 20 }))).total).toBe(7);
    expect(value(await page.evaluate(() => window.shellfox.getHistory({ search: '_%', status: 'all', page: 1, pageSize: 20 }))).total).toBe(0);
    const injection = value(await page.evaluate(() => window.shellfox.getHistory({ search: "' OR 1=1 --", status: 'all', page: 1, pageSize: 20 })));
    expect(injection.total).toBe(0);
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    // History page size is deliberately no longer an editable UI preference.
    // Autosaving appearance must preserve it.
    value(await page.evaluate(async () => {
      const current = await window.shellfox.getSnapshot();
      if (!current.ok) throw new Error('snapshot failed');
      return window.shellfox.saveSettings({ ...current.value.settings, historyPageSize: 3 });
    }));
    await page.getByRole('button', { name: 'Use #2dd4bf', exact: true }).click();
    await expect.poll(async () => (await snapshot(page)).settings.accentColor).toBe('#2dd4bf');
    await page.getByRole('button', { name: 'Close dialog', exact: true }).click();
    const identities = await app.evaluate(() => (globalThis as any).__shellfoxTest.backend.watch.map((t: any) => t.registration));
    const pragmas = await app.evaluate(() => {
      const db = (globalThis as any).__shellfoxTest.repository.db;
      return [db.pragma('user_version', { simple: true }), db.pragma('foreign_keys', { simple: true }), db.pragma('journal_mode', { simple: true })];
    });
    expect(pragmas).toEqual([3, 1, 'wal']);
    await app.close(); closed = true;
    ({ app, page } = await launch(data)); closed = false;
    expect((await snapshot(page)).settings.accentColor).toBe('#2dd4bf');
    expect((await snapshot(page)).settings.historyPageSize).toBe(3);
    expect((await calls(app)).launches).toBe(0);
    expect(await app.evaluate(() => (globalThis as any).__shellfoxTest.backend.watch.map((t: any) => t.registration))).toEqual(identities);
    const unknown = value(await page.evaluate(() => window.shellfox.getHistory({ search: '', status: 'unknown', page: 1, pageSize: 20 })));
    expect(unknown.total).toBe(7);
    expect(unknown.items.every(s => s.status === 'settled' && s.activityStatus === 'unknown')).toBe(true);
    await observe(app, 1);
    expect(value(await page.evaluate(() => window.shellfox.getHistory({ search: '', status: 'running', page: 1, pageSize: 20 }))).total).toBe(7);
    const redelivery = value(await page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, requestId }));
    expect(redelivery.id).toBe(original.id);
    expect((await calls(app)).launches).toBe(0);
    value(await page.evaluate(sessionId => window.shellfox.unsettleSession({ sessionId }), original.id));
    expect((await snapshot(page)).sessions.map(s => s.id)).toEqual([original.id]);
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
