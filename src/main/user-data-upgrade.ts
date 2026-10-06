import path from 'node:path';
import { constants } from 'node:fs';
import { access, copyFile, cp, link, mkdir, rm, writeFile } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import Database from 'better-sqlite3';

// Compatibility names only. Never move or delete the previous installation's data.
export const LEGACY_DATA_NAMES = ['Pi Manager', 'pi-manager'];
export const DATABASE_NAME = 'manager.sqlite3';
const MARKER = 'shellfox-upgrade.json';
async function exists(file: string): Promise<boolean> {
  try { await access(file); return true; }
  catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return false; throw error; }
}
export type DatabaseCopy = (source: string, destination: string) => Promise<void>;
const copyDatabase: DatabaseCopy = async (source, destination) => {
  const db = new Database(source, { readonly: true, fileMustExist: true, timeout: 5000 });
  const deadline = Date.now() + 15000;
  try {
    await db.backup(destination, { progress: () => { if (Date.now() > deadline) throw new Error('Shellfox database copy timed out. Original data was not changed.'); return 256; } });
  } finally { db.close(); }
};
export async function migrateUserData(appData: string, destination: string, log = console.info, backup: DatabaseCopy = copyDatabase): Promise<boolean> {
  if (await exists(path.join(destination, DATABASE_NAME)) || await exists(path.join(destination, MARKER))) return false;
  for (const name of LEGACY_DATA_NAMES) {
    const source = path.join(appData, name), database = path.join(source, DATABASE_NAME);
    if (path.resolve(source) === path.resolve(destination) || !await exists(database)) continue;
    await mkdir(destination, { recursive: true });
    // Permanent recovery copies retain the original DB/WAL/SHM. They are never opened.
    // SQLite backup reads the LIVE database, including committed WAL pages, into
    // a standalone snapshot. Opening a raw file copy while the old app writes is unsafe.
    const recovery = path.join(destination, 'legacy-backup');
    await mkdir(recovery, { recursive: true });
    for (const suffix of ['', '-wal', '-shm']) {
      try { await copyFile(database + suffix, path.join(recovery, DATABASE_NAME + suffix)); }
      catch (error) { if (suffix === '' || (error as NodeJS.ErrnoException).code !== 'ENOENT') throw error; }
    }
    for (const file of ['Preferences', 'Local State', 'settings.json', 'config.json']) {
      try { await copyFile(path.join(source, file), path.join(destination, file), constants.COPYFILE_EXCL); }
      catch (error) { if (!['ENOENT', 'EEXIST'].includes((error as NodeJS.ErrnoException).code ?? '')) throw error; }
    }
    // Preserve the renderer's storage so its old keys can be migrated on read.
    if (await exists(path.join(source, 'Local Storage')) && !await exists(path.join(destination, 'Local Storage'))) {
      await cp(path.join(source, 'Local Storage'), path.join(destination, 'Local Storage'), { recursive: true, force: false, filter: file => path.basename(file) !== 'LOCK' });
    }
    const staging = path.join(destination, 'tmp', `shellfox-upgrade-${randomUUID()}.sqlite3`);
    await mkdir(path.dirname(staging), { recursive: true });
    await rm(staging, { force: true });
    try {
      await backup(database, staging);
      // Same-volume hard-link publication is atomic and refuses an existing file.
      // Startup and installer migration can overlap; rename could replace a new DB.
      try { await link(staging, path.join(destination, DATABASE_NAME)); }
      catch (error) {
        if ((error as NodeJS.ErrnoException).code !== 'EEXIST') throw error;
        return false;
      }
    } finally { await rm(staging, { force: true }); }
    await writeFile(path.join(destination, MARKER), JSON.stringify({ source, copiedAt: new Date().toISOString(), database: DATABASE_NAME }) + '\n');
    log(`Shellfox: copied user data from ${source} to ${destination}. The original data was kept.`);
    return true;
  }
  return false;
}
