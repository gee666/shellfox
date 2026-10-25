import { test, expect } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import type { OperationRecord, SessionRecord, TabRecord } from '../../src/main/models';
import type { OwnedTerminal } from '../../src/main/terminal/backend';
import { launch, scratch, snapshot, value } from '../fixtures/electron';

interface RetainedRows {
  session: SessionRecord | null;
  tabs: TabRecord[];
  operations: OperationRecord[];
  metadata: { tabId: string }[];
  ownership: { key: string }[];
  live: string[];
}

test('real PTYs and SQLite keep settled tabs alive and delete the retained session across restart', async () => {
  const dir = await scratch('database-retention'), data = path.join(dir, 'data');
  let running = await launch(data, 'real');
  const rows = (id: string) => running.app.evaluate((_electron, id) => new Promise<RetainedRows>((resolve, reject) => setImmediate(() => {
    try {
      const r = (globalThis as any).__shellfoxTest.repository, db = r.db;
      resolve({
        session: r.session(id) ?? null, tabs: r.tabs(id),
        operations: db.prepare('SELECT * FROM operations WHERE sessionId=?').all(id),
        metadata: db.prepare('SELECT tabId FROM terminal_metadata').all(),
        ownership: db.prepare("SELECT key FROM preferences WHERE key LIKE 'tab-member:%' OR key LIKE 'session-window:%'").all(),
        live: (globalThis as any).__shellfoxTest.backend.live().filter((t: OwnedTerminal) => t.sessionId === id).map((t: OwnedTerminal) => t.tabId),
      });
    } catch (error) { reject(error); }
  })), id);
  try {
    const created = value(await running.page.evaluate(input => window.shellfox.createSession(input), { cwd: dir, title: 'Retained task', requestId: randomUUID() }));
    const added = value(await running.page.evaluate(input => window.shellfox.addTab(input), { sessionId: created.id }));
    const closed = added.tabs[0];
    value(await running.page.evaluate(input => window.shellfox.closeTab!(input), { tabId: closed.id, generation: closed.generation! }));
    let stored = await rows(created.id);
    expect(stored.tabs.map(t => t.id)).toEqual([added.tabs[1].id]);
    expect(stored.operations.some(o => o.tabId === closed.id)).toBe(false);
    expect(stored.metadata.some(t => t.tabId === closed.id)).toBe(false);
    value(await running.page.evaluate(input => window.shellfox.setSessionEnv(input), { sessionId: created.id, env: [{ name: 'CUSTOM_RETENTION', value: 'kept' }] }));
    const before = await rows(created.id);
    const settled = value(await running.page.evaluate(input => window.shellfox.settleSession(input), { sessionId: created.id, confirmActive: false }));
    expect(settled).toMatchObject({ status: 'settled', settledAt: expect.any(String) });
    expect(settled.tabs.map(t => t.id)).toEqual(before.tabs.map(t => t.id));
    stored = await rows(created.id);
    expect(stored.session).toMatchObject({ id: created.id, title: 'Retained task', env: [{ name: 'CUSTOM_RETENTION', value: 'kept' }], settledAt: expect.any(String) });
    expect(stored.tabs).toEqual(before.tabs);
    expect(stored.operations).toEqual(before.operations);
    expect(stored.metadata).toEqual(before.metadata);
    expect(stored.ownership).toEqual(before.ownership);
    expect(stored.live).toEqual(before.live);
    expect(stored.live).toEqual([added.tabs[1].id]);
    const current = (await snapshot(running.page)).sessions;
    expect(current).toHaveLength(1);
    expect(current[0]).toMatchObject({ id: created.id, status: 'settled', settledAt: settled.settledAt });
    expect(current[0].tabs.map(t => t.id)).toEqual(before.tabs.map(t => t.id));

    const unmarked = value(await running.page.evaluate(input => window.shellfox.unsettleSession(input), { sessionId: created.id }));
    expect(unmarked.settledAt).toBeNull();
    expect(unmarked.status).not.toBe('settled');
    expect((await rows(created.id)).tabs).toEqual(before.tabs);
    value(await running.page.evaluate(input => window.shellfox.settleSession(input), { sessionId: created.id, confirmActive: false }));
    await running.app.close(); running = await launch(data, 'real');
    const restored = (await snapshot(running.page)).sessions;
    expect(restored).toHaveLength(1);
    expect(restored[0]).toMatchObject({ id: created.id, status: 'settled', settledAt: expect.any(String) });
    // Shutdown closes app-owned shells. Restart must not replay their commands.
    expect(restored[0].tabs).toEqual([]);
    stored = await rows(created.id);
    expect(stored.tabs).toEqual([]); expect(stored.operations).toEqual([]);
    expect(stored.metadata).toEqual([]); expect(stored.live).toEqual([]);
    value(await running.page.evaluate(input => window.shellfox.deleteSession(input), { sessionId: created.id }));
    stored = await rows(created.id);
    expect(stored.session).toBeNull(); expect(stored.tabs).toEqual([]); expect(stored.operations).toEqual([]);
    expect(stored.metadata).toEqual([]); expect(stored.ownership).toEqual([]); expect(stored.live).toEqual([]);
    expect((await snapshot(running.page)).sessions).toEqual([]);
    await running.app.close(); running = await launch(data, 'real');
    stored = await rows(created.id);
    expect(stored.session).toBeNull(); expect(stored.tabs).toEqual([]); expect(stored.operations).toEqual([]);
    expect(stored.metadata).toEqual([]); expect(stored.ownership).toEqual([]); expect(stored.live).toEqual([]);
  } finally { await running.app.close(); }
});
