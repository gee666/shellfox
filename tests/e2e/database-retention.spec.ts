import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { launch, scratch, snapshot, value } from '../fixtures/electron';

test('real PTYs and SQLite remove closed tabs, archive only session data, and permanently delete it across restart', async () => {
  const dir = await scratch('database-retention'), data = path.join(dir, 'data');
  let running = await launch(data, 'real');
  const rows = (id: string) => running.app.evaluate((_electron, id) => new Promise<any>((resolve, reject) => setImmediate(() => {
    try {
    const r = (globalThis as any).__shellfoxTest.repository, db = r.db;
    resolve({
      session: r.session(id) ?? null, tabs: r.tabs(id),
      operations: db.prepare('SELECT * FROM operations WHERE sessionId=?').all(id),
      metadata: db.prepare('SELECT * FROM terminal_metadata').all(),
      ownership: db.prepare("SELECT key FROM preferences WHERE key LIKE 'tab-member:%' OR key LIKE 'session-window:%'").all(),
      live: (globalThis as any).__shellfoxTest.backend.live().filter((t: any) => t.sessionId === id).map((t: any) => t.tabId),
    });
    } catch (error) { reject(error); }
  })), id);
  try {
    const created = value(await running.page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, title: 'Retained task', requestId: randomUUID() }));
    const added = value(await running.page.evaluate(input => window.shellfox.addTab(input), { sessionId: created.id }));
    const closed = added.tabs[0];
    value(await running.page.evaluate(input => window.shellfox.closeTab!(input), { tabId: closed.id, generation: closed.generation! }));
    let stored = await rows(created.id);
    expect(stored.tabs.map((t: any) => t.id)).toEqual([added.tabs[1].id]);
    expect(stored.operations.some((o: any) => o.tabId === closed.id)).toBe(false);
    expect(stored.metadata.some((t: any) => t.tabId === closed.id)).toBe(false);
    value(await running.page.evaluate(input => window.shellfox.setSessionEnv!(input), { sessionId: created.id, env: [{ name: 'CUSTOM_RETENTION', value: 'kept' }] }));
    const archived = value(await running.page.evaluate(input => window.shellfox.settleSession(input), { sessionId: created.id, confirmActive: true }));
    expect(archived.tabs).toEqual([]);
    stored = await rows(created.id);
    expect(stored.session).toMatchObject({ id: created.id, title: 'Retained task', env: [{ name: 'CUSTOM_RETENTION', value: 'kept' }], settledAt: expect.any(String) });
    expect(stored.tabs).toEqual([]); expect(stored.operations).toEqual([]); expect(stored.metadata).toEqual([]); expect(stored.ownership).toEqual([]); expect(stored.live).toEqual([]);
    await running.app.close(); running = await launch(data, 'real');
    expect((await snapshot(running.page)).sessions).toEqual([]);
    expect((await rows(created.id)).tabs).toEqual([]);
    expect(value(await running.page.evaluate(() => window.shellfox.getHistory({ search: '', status: 'all', page: 1, pageSize: 20 }))).items[0]).toMatchObject({ id: created.id, tabs: [] });
    value(await running.page.evaluate(input => window.shellfox.deleteSession(input), { sessionId: created.id }));
    expect((await rows(created.id)).session).toBeNull();
    await running.app.close(); running = await launch(data, 'real');
    stored = await rows(created.id);
    expect(stored.session).toBeNull(); expect(stored.tabs).toEqual([]); expect(stored.operations).toEqual([]); expect(stored.metadata).toEqual([]); expect(stored.ownership).toEqual([]);
  } finally { await running.app.close(); }
});
