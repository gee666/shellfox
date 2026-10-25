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
    const session = value(await old.page.evaluate(cwd => window.shellfox.createSession({ cwd, title: 'Upgrade session', requestId: crypto.randomUUID() }), dir));
    value(await old.page.evaluate(sessionId => window.shellfox.settleSession({ sessionId, confirmActive: false }), session.id));
    // CDP evaluation can interrupt a native SQLite query. Defer DB access to the event loop.
    await old.app.evaluate(() => new Promise<void>((resolve, reject) => setImmediate(() => {
      try {
      const repository = (globalThis as any).__shellfoxTest.repository;
      repository.db.pragma('wal_autocheckpoint = 0');
      repository.saveSettings({ ...repository.settings(), accentColor: '#2dd4bf' });
      repository.saveExplorerPreference(true);
      repository.saveCliPreference(true);
      resolve();
      } catch (error) { reject(error); }
    })));
    expect((await stat(path.join(oldData, 'manager.sqlite3-wal'))).size).toBeGreaterThan(0);
    await writeFile(path.join(oldData, 'settings.json'), '{"fixture":"preserved"}');
    await old.page.evaluate(() => localStorage.setItem('pi-manager.sidebarWidth', '280'));
    const openNew = () => _electron.launch({ cwd: root, env: { ...env(), SHELLFOX_TEST_APP_DATA: dir }, args: [testMain, '--test-user-data', newData, '--test-backend', 'fake'], timeout: 30000 });
    current = await openNew();
    let page = await current.firstWindow(); await page.waitForFunction(() => !!window.shellfox);
    // The built document title uses the product manifest, not the loose Electron entry's runtime version.
    const version = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8')).version;
    expect(await page.title()).toBe(`Shellfox v${version}`);
    expect(await current.evaluate(({ app }) => [app.getName(), app.getPath('userData')])).toEqual(['Shellfox', newData]);
    expect((await snapshot(page)).settings.accentColor).toBe('#2dd4bf');
    expect((await snapshot(page)).sessions.find(s => s.id === session.id)).toMatchObject({ title: 'Upgrade session', status: 'settled', settledAt: expect.any(String), tabs: session.tabs.map(t => expect.objectContaining({ id: t.id })) });
    expect(await current.evaluate(() => new Promise<unknown[]>((resolve, reject) => setImmediate(() => {
      try {
        const r = (globalThis as any).__shellfoxTest.repository;
        resolve([r.explorerPreference(), r.cliPreference(), r.db.pragma('integrity_check', { simple: true })]);
      } catch (error) { reject(error); }
    })))).toEqual([true, true, 'ok']);
    expect(await readFile(path.join(newData, 'settings.json'), 'utf8')).toBe('{"fixture":"preserved"}');
    for (const suffix of ['', '-wal', '-shm']) expect((await stat(path.join(newData, 'legacy-backup', 'manager.sqlite3' + suffix))).size).toBeGreaterThan(0);
    // The original app still has its session and tabs. Migration never stops its processes.
    expect((await snapshot(old.page)).sessions).toHaveLength(1);
    expect((await snapshot(old.page)).sessions[0]).toMatchObject({ id: session.id, status: 'settled', tabs: session.tabs.map(t => expect.objectContaining({ id: t.id })) });
    await page.evaluate(async () => {
      const s = await window.shellfox.getSnapshot(); if (!s.ok) throw Error('snapshot');
      await window.shellfox.saveSettings({ ...s.value.settings, accentColor: '#60a5fa' });
    });
    await current.close(); current = await openNew(); page = await current.firstWindow(); await page.waitForFunction(() => !!window.shellfox);
    expect((await snapshot(page)).settings.accentColor).toBe('#60a5fa');
    expect((await snapshot(old.page)).settings.accentColor).toBe('#2dd4bf');
  } finally { await current?.close(); await old.app.close(); }
});
