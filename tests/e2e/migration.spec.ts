import { test, expect } from '@playwright/test';
import path from 'node:path';
import { launch, scratch } from '../fixtures/electron';

test('failed v0 migration rolls back partial DDL; future schema is refused without modifying data', async () => {
  const dir = await scratch('migration');
  const { app } = await launch(path.join(dir, 'data'));
  try {
    const result = await app.evaluate((_electron, filenames) => {
      const repository = (globalThis as any).__shellfoxTest.repository;
      const Repository = repository.constructor;
      const Database = repository.db.constructor;
      const collision = new Database(filenames[0]);
      collision.exec('CREATE TABLE tabs (sentinel TEXT); INSERT INTO tabs VALUES (\'preserved\')');
      collision.close();
      let migrationRejected = false;
      try { new Repository(filenames[0]); } catch { migrationRejected = true; }
      const reopened = new Database(filenames[0]);
      const version = reopened.pragma('user_version', { simple: true });
      const tables = reopened.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name").all().map((r: any) => r.name);
      const sentinel = reopened.prepare('SELECT sentinel FROM tabs').get().sentinel;
      reopened.close();
      const future = new Database(filenames[1]); future.pragma('user_version=4'); future.close();
      let futureRejected = false;
      try { new Repository(filenames[1]); } catch { futureRejected = true; }
      const unchanged = new Database(filenames[1]);
      const futureVersion = unchanged.pragma('user_version', { simple: true }); unchanged.close();
      return { migrationRejected, version, tables, sentinel, futureRejected, futureVersion };
    }, [path.join(dir, 'collision.sqlite3'), path.join(dir, 'future.sqlite3')]);
    expect(result).toEqual({ migrationRejected: true, version: 0, tables: ['tabs'], sentinel: 'preserved', futureRejected: true, futureVersion: 4 });
  } finally { await app.close(); }
});
