import Database from 'better-sqlite3';
import type { SettingsDto } from '../shared/contracts';
import { settingsSchema, storedEnvVarsSchema } from '../shared/schemas';
import type { RepositoryPort, SessionRecord, TabRecord, OperationRecord } from './models';
import { defaultSettings, upgradeBundledRules } from './defaults';

/** Latest schema migration. Opening a database with a higher user_version is refused. */
export const SCHEMA_VERSION = 5;
export class Repository implements RepositoryPort {
  readonly db: Database.Database;
  constructor(filename: string) {
    this.db = new Database(filename);
    try {
      this.db.pragma('journal_mode = WAL');
      this.db.pragma('foreign_keys = ON');
      this.db.pragma('busy_timeout = 5000');
      const version = this.db.pragma('user_version', { simple: true }) as number;
      if (version > SCHEMA_VERSION) throw new Error('Database was created by a newer app');
      if (version === 0) this.db.transaction(() => {
        this.db.exec(`
          CREATE TABLE sessions (
            id TEXT PRIMARY KEY, title TEXT NOT NULL, cwd TEXT NOT NULL,
            adapterId TEXT NOT NULL, shellId TEXT NOT NULL, shellExecutable TEXT NOT NULL,
            createdAt TEXT NOT NULL, updatedAt TEXT NOT NULL, settledAt TEXT,
            error TEXT, target TEXT
          );
          CREATE TABLE tabs (
            id TEXT PRIMARY KEY, sessionId TEXT NOT NULL REFERENCES sessions(id),
            title TEXT NOT NULL, cwd TEXT NOT NULL, ordinal INTEGER NOT NULL,
            createdAt TEXT NOT NULL, lifecycle TEXT NOT NULL, operationId TEXT NOT NULL,
            registration TEXT, error TEXT, UNIQUE(sessionId, ordinal)
          );
          CREATE TABLE operations (
            id TEXT PRIMARY KEY, sessionId TEXT NOT NULL REFERENCES sessions(id),
            tabId TEXT REFERENCES tabs(id), requestId TEXT UNIQUE, kind TEXT NOT NULL,
            state TEXT NOT NULL, createdAt TEXT NOT NULL, updatedAt TEXT NOT NULL, error TEXT
          );
          CREATE TABLE settings (id INTEGER PRIMARY KEY CHECK(id=1), value TEXT NOT NULL);
          CREATE TABLE preferences (key TEXT PRIMARY KEY, value TEXT NOT NULL);
          CREATE INDEX session_tabs ON tabs(sessionId, ordinal);
        `);
        this.db.prepare('INSERT INTO settings VALUES (1, ?)').run(JSON.stringify(defaultSettings));
        this.db.pragma('user_version = 1');
      })();
      // Version 2 stores embedded ownership metadata separately; legacy roots remain untouched.
      if (version < 2) this.db.transaction(() => {
        this.db.exec('CREATE TABLE IF NOT EXISTS terminal_metadata (tabId TEXT PRIMARY KEY REFERENCES tabs(id), value TEXT NOT NULL)');
        this.db.pragma('user_version = 2');
      })();
      if (version < 3) this.db.transaction(() => {
        this.db.exec("ALTER TABLE sessions ADD COLUMN env TEXT NOT NULL DEFAULT '[]'");
        this.db.pragma('user_version = 3');
      })();
      // Version 4 adds session pins.
      if (version < 4) this.db.transaction(() => {
        this.db.exec('ALTER TABLE sessions ADD COLUMN pinnedAt TEXT');
        this.db.pragma('user_version = 4');
      })();
      // Version 5 removes obsolete tab/launch records, never their processes.
      if (version < 5) this.transaction(() => {
        for (const row of this.db.prepare("SELECT id FROM tabs WHERE lifecycle='closed' OR sessionId NOT IN (SELECT id FROM sessions)").all() as { id: string }[]) this.deleteTab(row.id);
        this.db.exec(`
          DELETE FROM operations WHERE sessionId NOT IN (SELECT id FROM sessions)
            OR (tabId IS NOT NULL AND tabId NOT IN (SELECT id FROM tabs));
          DELETE FROM terminal_metadata WHERE tabId NOT IN (SELECT id FROM tabs);
          DELETE FROM preferences WHERE
            (substr(key,1,11)='tab-member:' AND substr(key,12) NOT IN (SELECT id FROM tabs)) OR
            (substr(key,1,15)='session-window:' AND substr(key,16) NOT IN (SELECT id FROM sessions));
        `);
        this.db.pragma('user_version = 5');
      });
      const settings = this.settings();
      const upgraded = upgradeBundledRules(settings);
      if (upgraded !== settings) this.saveSettings(upgraded);
    } catch (error) {
      this.db.close();
      throw error;
    }
  }
  transaction<T>(fn: () => T): T { return this.db.transaction(fn)(); }
  private decode<T>(row: unknown, fields: string[]): T | undefined {
    if (!row) return undefined;
    const record = { ...(row as Record<string, unknown>) };
    for (const field of fields) record[field] = record[field] ? JSON.parse(record[field] as string) : null;
    if (fields.includes('target')) {
      record.env = storedEnvVarsSchema.parse(JSON.parse((record.env as string) ?? '[]'));
      Object.assign(record, this.metadata('session-window:' + record.id) ?? { binding: null, windowState: 'unknown' });
    }
    if (fields.includes('registration')) {
      record.member = (this.metadata('tab-member:' + record.id) as { member?: unknown } | undefined)?.member ?? null;
      const terminal = this.db.prepare('SELECT value FROM terminal_metadata WHERE tabId=?').get(record.id) as { value: string } | undefined;
      record.terminal = terminal ? JSON.parse(terminal.value) : null;
    }
    return record as T;
  }
  private metadata(key: string): unknown {
    const row = this.db.prepare('SELECT value FROM preferences WHERE key=?').get(key) as { value: string } | undefined;
    return row ? JSON.parse(row.value) : undefined;
  }
  private saveMetadata(key: string, value: unknown): void {
    this.db.prepare('INSERT INTO preferences VALUES (?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value').run(key, JSON.stringify(value));
  }
  windowState(): unknown { return this.metadata('window-state'); }
  saveWindowState(value: unknown): void { this.saveMetadata('window-state', value); }
  sessions(): SessionRecord[] { return this.db.prepare('SELECT * FROM sessions').all().map(r => this.decode<SessionRecord>(r, ['error', 'target'])!); }
  session(id: string): SessionRecord | undefined { return this.decode(this.db.prepare('SELECT * FROM sessions WHERE id=?').get(id), ['error', 'target']); }
  tabs(sessionId?: string): TabRecord[] {
    const rows = sessionId ? this.db.prepare('SELECT * FROM tabs WHERE sessionId=? ORDER BY ordinal, id').all(sessionId) : this.db.prepare('SELECT * FROM tabs ORDER BY sessionId, ordinal, id').all();
    return rows.map(r => this.decode<TabRecord>(r, ['registration', 'error'])!);
  }
  tab(id: string): TabRecord | undefined { return this.decode(this.db.prepare('SELECT * FROM tabs WHERE id=?').get(id), ['registration', 'error']); }
  saveSession(s: SessionRecord): void {
    this.transaction(() => {
    this.db.prepare(`INSERT INTO sessions (id,title,cwd,adapterId,shellId,shellExecutable,createdAt,updatedAt,settledAt,error,target,env,pinnedAt) VALUES (@id,@title,@cwd,@adapterId,@shellId,@shellExecutable,@createdAt,@updatedAt,@settledAt,@error,@target,@env,@pinnedAt)
      ON CONFLICT(id) DO UPDATE SET title=excluded.title,adapterId=excluded.adapterId,shellId=excluded.shellId,shellExecutable=excluded.shellExecutable,updatedAt=excluded.updatedAt,settledAt=excluded.settledAt,error=excluded.error,target=excluded.target,env=excluded.env,pinnedAt=excluded.pinnedAt`).run({ ...s, pinnedAt: s.pinnedAt ?? null, env: JSON.stringify(storedEnvVarsSchema.parse(s.env ?? [])), error: encode(s.error), target: encode(s.target) });
    if (s.binding !== undefined) this.saveMetadata('session-window:' + s.id, { binding: s.binding, windowState: s.windowState ?? 'unknown' });
    });
  }
  saveTab(t: TabRecord): void {
    this.transaction(() => {
    this.db.prepare(`INSERT INTO tabs VALUES (@id,@sessionId,@title,@cwd,@ordinal,@createdAt,@lifecycle,@operationId,@registration,@error)
      ON CONFLICT(id) DO UPDATE SET sessionId=excluded.sessionId,title=excluded.title,ordinal=excluded.ordinal,cwd=excluded.cwd,lifecycle=excluded.lifecycle,operationId=excluded.operationId,registration=excluded.registration,error=excluded.error`).run({ ...t, registration: encode(t.registration), error: encode(t.error) });
    if (t.member !== undefined) this.saveMetadata('tab-member:' + t.id, { member: t.member });
    if (t.terminal !== undefined) {
      if (t.terminal) this.db.prepare('INSERT INTO terminal_metadata VALUES (?,?) ON CONFLICT(tabId) DO UPDATE SET value=excluded.value').run(t.id, JSON.stringify(t.terminal));
      else this.db.prepare('DELETE FROM terminal_metadata WHERE tabId=?').run(t.id);
    }
    });
  }
  deleteTab(id: string): boolean {
    return this.transaction(() => {
      this.db.prepare('DELETE FROM operations WHERE tabId=? OR id=(SELECT operationId FROM tabs WHERE id=?)').run(id, id);
      this.db.prepare('DELETE FROM terminal_metadata WHERE tabId=?').run(id);
      this.db.prepare('DELETE FROM preferences WHERE key=?').run('tab-member:' + id);
      return this.db.prepare('DELETE FROM tabs WHERE id=?').run(id).changes > 0;
    });
  }
  deleteSession(id: string): boolean {
    return this.transaction(() => {
      const session = this.db.prepare('SELECT id FROM sessions WHERE id=?').get(id);
      if (!session) return false;
      const tabIds = (this.db.prepare('SELECT id FROM tabs WHERE sessionId=?').all(id) as { id: string }[]).map(tab => tab.id);
      this.db.prepare('DELETE FROM operations WHERE sessionId=?').run(id);
      for (const tabId of tabIds) {
        this.db.prepare('DELETE FROM terminal_metadata WHERE tabId=?').run(tabId);
        this.db.prepare('DELETE FROM preferences WHERE key=?').run('tab-member:' + tabId);
      }
      this.db.prepare('DELETE FROM tabs WHERE sessionId=?').run(id);
      this.db.prepare('DELETE FROM preferences WHERE key=?').run('session-window:' + id);
      this.db.prepare('DELETE FROM sessions WHERE id=?').run(id);
      return true;
    });
  }
  saveOperation(o: OperationRecord): void {
    this.db.prepare(`INSERT INTO operations VALUES (@id,@sessionId,@tabId,@requestId,@kind,@state,@createdAt,@updatedAt,@error)
      ON CONFLICT(id) DO UPDATE SET sessionId=excluded.sessionId,tabId=excluded.tabId,state=excluded.state,updatedAt=excluded.updatedAt,error=excluded.error`).run({ ...o, error: encode(o.error) });
  }
  operation(id: string): OperationRecord | undefined { return this.decode(this.db.prepare('SELECT * FROM operations WHERE id=?').get(id), ['error']); }
  operationsForTab(tabId: string): OperationRecord[] {
    return this.db.prepare('SELECT * FROM operations WHERE tabId=?').all(tabId).map(row => this.decode<OperationRecord>(row, ['error'])!);
  }
  sessionByRequest(requestId: string): SessionRecord | undefined {
    return this.decode(this.db.prepare('SELECT sessions.* FROM sessions JOIN operations ON sessions.id=operations.sessionId WHERE operations.requestId=?').get(requestId), ['error', 'target']);
  }
  settings(): SettingsDto { return settingsSchema.parse(JSON.parse((this.db.prepare('SELECT value FROM settings WHERE id=1').get() as { value: string }).value)); }
  saveSettings(settings: SettingsDto): void { this.db.prepare('UPDATE settings SET value=? WHERE id=1').run(JSON.stringify(settings)); }
  explorerPreference(): boolean { return (this.db.prepare("SELECT value FROM preferences WHERE key='explorer'").get() as { value: string } | undefined)?.value === 'true'; }
  saveExplorerPreference(installed: boolean): void { this.db.prepare("INSERT INTO preferences VALUES ('explorer',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").run(JSON.stringify(installed)); }
  cliPreference(): boolean | null {
    const row = this.db.prepare("SELECT value FROM preferences WHERE key='shellfox-cli'").get() as { value: string } | undefined;
    return row ? row.value === 'true' : null;
  }
  saveCliPreference(installed: boolean): void { this.db.prepare("INSERT INTO preferences VALUES ('shellfox-cli',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").run(JSON.stringify(installed)); }
  close(): void { this.db.close(); }
}
function encode(value: unknown): string | null { return value === null ? null : JSON.stringify(value); }
