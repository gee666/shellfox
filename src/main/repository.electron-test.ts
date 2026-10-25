// Runs in Electron, not Vitest's host Node. All database files stay under project tmp.
import { app } from 'electron';
import assert from 'node:assert/strict';
import path from 'node:path';
import { mkdirSync } from 'node:fs';
import { randomUUID } from 'node:crypto';
import { Repository, SCHEMA_VERSION } from './repository';
import { EmbeddedSessionService } from './terminal/service';
import { defaultSettings } from './defaults';
import type { SessionRecord, TabRecord } from './models';
declare const __PROJECT_ROOT__: string;
const folder = path.join(__PROJECT_ROOT__, 'tmp', 'storage-tests', randomUUID());
mkdirSync(folder, { recursive: true });
app.setPath('userData', folder);
async function run() {
  await app.whenReady();
  const filename = path.join(folder, 'manager.sqlite3');
  let repository = new Repository(filename);
  const now = new Date().toISOString();
  const session = (extra: Partial<SessionRecord> = {}): SessionRecord => ({ id: randomUUID(), title: "100%_ 雪 ' OR 1=1 --", cwd: 'C:\\same', adapterId: 'embedded-pty', shellId: 'pwsh', shellExecutable: 'C:\\pwsh.exe', createdAt: now, updatedAt: now, settledAt: null, error: null, target: null, env: [{ name: 'CUSTOM', value: 'persisted' }], pinnedAt: now, binding: null, ...extra });
  const tab = (s: SessionRecord, ordinal = 0, extra: Partial<TabRecord> = {}): TabRecord => ({ id: randomUUID(), sessionId: s.id, title: `Shell ${ordinal + 1}`, cwd: s.cwd, ordinal, createdAt: now, lifecycle: 'open', operationId: randomUUID(), registration: null, error: null, member: null, terminal: { kind: 'embedded', profileId: 'login-shell', exitCode: null, userTitle: null }, ...extra });
  const seed = (s: SessionRecord, tabs: TabRecord[]) => repository.transaction(() => {
    repository.saveSession(s);
    for (const t of tabs) {
      repository.saveTab(t);
      repository.saveOperation({ id: t.operationId, sessionId: s.id, tabId: t.id, requestId: randomUUID(), kind: 'create', state: 'registered', createdAt: now, updatedAt: now, error: null });
    }
  });
  const count = (table: string, column: string, id: string) => (repository.db.prepare(`SELECT count(*) n FROM ${table} WHERE ${column}=?`).get(id) as { n: number }).n;
  const removed = (t: TabRecord) => {
    assert.equal(repository.tab(t.id), undefined);
    assert.equal(repository.operation(t.operationId), undefined);
    assert.equal(count('operations', 'tabId', t.id), 0);
    assert.equal(count('terminal_metadata', 'tabId', t.id), 0);
    assert.equal(count('preferences', 'key', 'tab-member:' + t.id), 0);
  };
  assert.equal(repository.db.pragma('user_version', { simple: true }), SCHEMA_VERSION);
  assert.equal(repository.db.pragma('journal_mode', { simple: true }), 'wal');
  assert.equal(repository.db.pragma('foreign_keys', { simple: true }), 1);
  assert.deepEqual(repository.settings(), defaultSettings);
  repository.saveSettings({ ...defaultSettings, accentColor: '#123456' });
  repository.saveExplorerPreference(true); repository.saveCliPreference(true);
  repository.saveWindowState({ x: 1, y: 2, width: 900, height: 700, maximized: true });
  const live = session(), a = tab(live), b = tab(live, 1);
  seed(live, [a, b]);
  assert.throws(() => repository.saveTab({ ...a, id: randomUUID(), sessionId: randomUUID() }));
  // Tab deletion is atomic, removes every retry operation, and does not affect siblings.
  repository.saveOperation({ ...repository.operation(a.operationId)!, id: randomUUID(), requestId: randomUUID(), kind: 'retry' });
  assert.throws(() => repository.transaction(() => { repository.deleteTab(a.id); throw new Error('rollback'); }));
  assert.ok(repository.tab(a.id)); assert.equal(repository.operationsForTab(a.id).length, 2);
  // Old session-only intents can still be referenced by the tab's operationId.
  repository.saveOperation({ ...repository.operation(a.operationId)!, tabId: null });
  assert.equal(repository.deleteTab(a.id), true); removed(a); assert.ok(repository.tab(b.id));
  assert.equal(repository.deleteTab(a.id), false);
  // Real SQLite title/order transactions retain open tabs until they close.
  const c = tab(live, 2); seed(live, [c]);
  const service = new EmbeddedSessionService(repository, undefined, () => ({ setWatch() {}, dispose() {} }));
  assert.equal((await service.renameTab({ sessionId: live.id, tabId: b.id, title: 'Build 雪' })).ok, true);
  assert.equal((await service.reorderTabs({ sessionId: live.id, tabIds: [c.id, b.id] })).ok, true);
  const before = repository.tabs(live.id), saveSession = repository.saveSession.bind(repository);
  repository.saveSession = () => { throw new Error('disk full'); };
  assert.equal((await service.renameTab({ sessionId: live.id, tabId: b.id, title: 'unsaved' })).ok, false);
  assert.equal((await service.reorderTabs({ sessionId: live.id, tabIds: [b.id, c.id] })).ok, false);
  assert.deepEqual(repository.tabs(live.id), before); repository.saveSession = saveSession;
  await service.dispose(); // No backend owns these fixture tabs.
  repository.close(); repository = new Repository(filename);
  assert.deepEqual(repository.tabs(live.id), before); assert.equal(repository.tab(b.id)?.terminal?.userTitle, 'Build 雪');
  // Settlement is a metadata edit and keeps tabs, launch records, env and pins.
  const settled = session(), d = tab(settled); seed(settled, [d]);
  repository.saveOperation({ id: randomUUID(), sessionId: settled.id, tabId: null, requestId: randomUUID(), kind: 'adopt', state: 'failed', createdAt: now, updatedAt: now, error: null });
  assert.throws(() => repository.transaction(() => { repository.saveSession({ ...settled, settledAt: now }); throw new Error('rollback'); }));
  assert.equal(repository.session(settled.id)?.settledAt, null); assert.ok(repository.tab(d.id));
  repository.saveSession({ ...settled, settledAt: now });
  assert.ok(repository.tab(d.id)); assert.equal(count('operations', 'sessionId', settled.id), 2);
  assert.deepEqual(repository.session(settled.id)?.env, settled.env); assert.equal(repository.session(settled.id)?.pinnedAt, now);
  repository.close(); repository = new Repository(filename);
  assert.ok(repository.tab(d.id)); assert.equal(repository.session(settled.id)?.settledAt, now);
  // Both settled and unsettled sessions can be deleted atomically.
  const direct = session(), directTab = tab(direct); seed(direct, [directTab]);
  assert.equal(repository.deleteSession(direct.id), true); removed(directTab);
  assert.equal(repository.deleteSession(randomUUID()), false);
  assert.throws(() => repository.transaction(() => { repository.deleteSession(settled.id); throw new Error('rollback'); }));
  assert.ok(repository.session(settled.id)); assert.ok(repository.tab(d.id));
  assert.equal(repository.deleteSession(settled.id), true); removed(d);
  assert.equal(count('operations', 'sessionId', settled.id), 0);
  assert.equal(count('preferences', 'key', 'session-window:' + settled.id), 0);
  assert.equal(repository.deleteSession(settled.id), false); assert.deepEqual(repository.tabs(live.id), before);
  // Version 4 data: closed and archived tabs, launch requests, dangling legacy preferences.
  const oldArchive = session({ settledAt: now, adapterId: 'windows-terminal' }), oldTab = tab(oldArchive, 0, { terminal: null });
  const oldClosed = tab(live, 3, { lifecycle: 'closed' }); seed(oldArchive, [oldTab]); seed(live, [oldClosed]);
  repository.db.prepare('INSERT INTO preferences VALUES (?,?)').run('tab-member:missing-tab', '{"member":null}');
  repository.db.prepare('INSERT INTO preferences VALUES (?,?)').run('session-window:missing-session', '{"binding":null}');
  // Simulate old writes made with foreign-key checks disabled.
  const orphan = tab(session());
  repository.db.pragma('foreign_keys = OFF');
  repository.saveTab(orphan);
  repository.saveOperation({ id: orphan.operationId, sessionId: orphan.sessionId, tabId: orphan.id, requestId: randomUUID(), kind: 'create', state: 'failed', createdAt: now, updatedAt: now, error: null });
  repository.db.prepare('INSERT INTO terminal_metadata VALUES (?,?)').run('missing-tab', '{}');
  repository.db.pragma('foreign_keys = ON');
  repository.db.pragma('user_version = 4');
  // A failed migration must roll back deletion and keep the previous schema version.
  repository.db.exec("CREATE TRIGGER refuse_retention BEFORE DELETE ON tabs BEGIN SELECT RAISE(ABORT, 'storage failure'); END");
  repository.close(); assert.throws(() => new Repository(filename), /storage failure/);
  // Open without migrations to inspect the rollback and remove only our test trigger.
  const Database = (await import('better-sqlite3')).default;
  const raw = new Database(filename);
  assert.equal(raw.pragma('user_version', { simple: true }), 4);
  assert.ok(raw.prepare('SELECT id FROM tabs WHERE id=?').get(oldClosed.id));
  assert.ok(raw.prepare('SELECT id FROM operations WHERE id=?').get(oldTab.operationId));
  raw.exec('DROP TRIGGER refuse_retention'); raw.close();
  repository = new Repository(filename); removed(oldClosed); assert.ok(repository.tab(oldTab.id)); removed(orphan);
  assert.equal(count('terminal_metadata', 'tabId', 'missing-tab'), 0);
  assert.ok(repository.session(oldArchive.id)); assert.deepEqual(repository.tabs(live.id), before);
  assert.equal(count('preferences', 'key', 'tab-member:missing-tab'), 0);
  assert.equal(count('preferences', 'key', 'session-window:missing-session'), 0);
  assert.equal(count('preferences', 'key', 'session-window:' + oldArchive.id), 1);
  assert.equal(repository.settings().accentColor, '#123456'); assert.equal(repository.explorerPreference(), true); assert.equal(repository.cliPreference(), true);
  assert.deepEqual(repository.windowState(), { x: 1, y: 2, width: 900, height: 700, maximized: true });
  assert.deepEqual(repository.db.pragma('foreign_key_check'), []);
  repository.close(); repository = new Repository(filename);
  assert.equal(repository.db.pragma('user_version', { simple: true }), SCHEMA_VERSION); removed(oldClosed); assert.ok(repository.tab(oldTab.id));
  // Older schema upgrades still work, including env and pin columns.
  repository.db.exec('DROP TABLE terminal_metadata; ALTER TABLE sessions DROP COLUMN env; ALTER TABLE sessions DROP COLUMN pinnedAt');
  repository.db.pragma('user_version = 1'); repository.close(); repository = new Repository(filename);
  assert.equal(repository.db.pragma('user_version', { simple: true }), SCHEMA_VERSION);
  assert.equal(repository.session(live.id)?.pinnedAt, null); assert.equal(repository.tab(b.id)?.terminal, null);
  const beforeCount = repository.sessions().length;
  for (let i = 0; i < 24; i++) repository.saveSession(session({ title: 'saved ' + i, settledAt: now }));
  assert.equal(repository.sessions().length, beforeCount + 24);
  assert.ok(repository.sessions().some(s => s.settledAt === now));
  repository.close();
  console.log('SQLite Electron storage: retention, rollback, migration and restart assertions passed; DB ' + filename);
  app.exit(0);
}
void run().catch(error => { console.error('SQLite Electron storage failed', error); app.exit(1); });
