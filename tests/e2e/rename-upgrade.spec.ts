import { test, expect, _electron } from '@playwright/test';
import path from 'node:path';
import { readFile, stat, writeFile } from 'node:fs/promises';
import { launch, scratch, env, root, testMain, snapshot, value } from '../fixtures/electron';

// Compatibility folder names are deliberate. Both old and new instances use isolated data.
for (const legacy of ['Pi Manager', 'pi-manager']) test(`Shellfox startup copies live ${legacy} WAL data once and keeps rollback data`, async () => {
  const dir = await scratch('rename-upgrade'), oldData = path.join(dir, legacy), newData = path.join(dir, 'Shellfox');
  const old = await launch(oldData);
  let current: Awaited<ReturnType<typeof _electron.launch>> | undefined;
  try {
    const session = value(await old.page.evaluate(cwd => window.shellfox.createSession({ cwd, title: 'Upgrade history', requestId: crypto.randomUUID() }), dir));
    value(await old.page.evaluate(sessionId => window.shellfox.settleSession({ sessionId, confirmActive: true }), session.id));
    await old.app.evaluate(() => {
      const repository = (globalThis as any).__shellfoxTest.repository;
      repository.db.pragma('wal_autocheckpoint = 0');
      repository.saveSettings({ ...repository.settings(), accentColor: '#2dd4bf' });
      repository.saveExplorerPreference(true);
      repository.saveCliPreference(true);
    });
    expect((await stat(path.join(oldData, 'manager.sqlite3-wal'))).size).toBeGreaterThan(0);
    await writeFile(path.join(oldData, 'settings.json'), '{"fixture":"preserved"}');
    await old.page.evaluate(() => localStorage.setItem('pi-manager.sidebarWidth', '280'));
    const openNew = () => _electron.launch({ cwd: root, env: { ...env(), SHELLFOX_TEST_APP_DATA: dir }, args: [testMain, '--test-user-data', newData, '--test-backend', 'fake'], timeout: 30000 });
    current = await openNew();
    let page = await current.firstWindow(); await page.waitForFunction(() => !!window.shellfox);
    expect(await page.title()).toBe('Shellfox');
    expect(await current.evaluate(({ app }) => [app.getName(), app.getPath('userData')])).toEqual(['Shellfox', newData]);
    expect((await snapshot(page)).settings.accentColor).toBe('#2dd4bf');
    expect(value(await page.evaluate(() => window.shellfox.getHistory({ search: 'Upgrade history', status: 'all', page: 1, pageSize: 20 }))).items[0].id).toBe(session.id);
    expect(await current.evaluate(() => {
      const r = (globalThis as any).__shellfoxTest.repository;
      return [r.explorerPreference(), r.cliPreference(), r.db.pragma('integrity_check', { simple: true })];
    })).toEqual([true, true, 'ok']);
    expect(await readFile(path.join(newData, 'settings.json'), 'utf8')).toBe('{"fixture":"preserved"}');
    for (const suffix of ['', '-wal', '-shm']) expect((await stat(path.join(newData, 'legacy-backup', 'manager.sqlite3' + suffix))).size).toBeGreaterThan(0);
    // Old app still has its history and remains usable. No process was stopped by migration.
    expect(value(await old.page.evaluate(() => window.shellfox.getHistory({ search: 'Upgrade history', status: 'all', page: 1, pageSize: 20 }))).total).toBe(1);
    await page.evaluate(async () => {
      const s = await window.shellfox.getSnapshot(); if (!s.ok) throw Error('snapshot');
      await window.shellfox.saveSettings({ ...s.value.settings, accentColor: '#60a5fa' });
    });
    await current.close(); current = await openNew(); page = await current.firstWindow(); await page.waitForFunction(() => !!window.shellfox);
    expect((await snapshot(page)).settings.accentColor).toBe('#60a5fa');
    expect((await snapshot(old.page)).settings.accentColor).toBe('#2dd4bf');
  } finally { await current?.close(); await old.app.close(); }
});
