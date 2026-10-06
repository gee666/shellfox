import { expect, it, vi } from 'vitest';
import path from 'node:path';
import { copyFile, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { migrateUserData, LEGACY_DATA_NAMES, DATABASE_NAME } from './user-data-upgrade';

it.each(LEGACY_DATA_NAMES)('copies %s data once without deleting or replacing either installation', async name => {
  await mkdir('tmp', { recursive: true });
  const root = await mkdtemp(path.resolve('tmp/rename-unit-'));
  const source = path.join(root, name), destination = path.join(root, 'Shellfox'), log = vi.fn();
  try {
    await mkdir(path.join(source, 'Local Storage'), { recursive: true });
    for (const suffix of ['', '-wal', '-shm']) await writeFile(path.join(source, DATABASE_NAME + suffix), 'old' + suffix);
    await writeFile(path.join(source, 'settings.json'), '{"saved":true}');
    await writeFile(path.join(source, 'Local Storage', 'fixture'), 'storage');
    const backup = vi.fn(copyFile);
    expect(await migrateUserData(root, destination, log, backup)).toBe(true);
    expect(backup).toHaveBeenCalledTimes(1);
    expect(await readFile(path.join(destination, DATABASE_NAME), 'utf8')).toBe('old');
    for (const suffix of ['', '-wal', '-shm']) {
      expect(await readFile(path.join(source, DATABASE_NAME + suffix), 'utf8')).toBe('old' + suffix);
      expect(await readFile(path.join(destination, 'legacy-backup', DATABASE_NAME + suffix), 'utf8')).toBe('old' + suffix);
    }
    expect(await readFile(path.join(destination, 'settings.json'), 'utf8')).toBe('{"saved":true}');
    expect(await readFile(path.join(destination, 'Local Storage', 'fixture'), 'utf8')).toBe('storage');
    expect(log).toHaveBeenCalledWith(expect.stringContaining('The original data was kept.'));
    await writeFile(path.join(destination, DATABASE_NAME), 'new');
    expect(await migrateUserData(root, destination, log, backup)).toBe(false);
    expect(await readFile(path.join(destination, DATABASE_NAME), 'utf8')).toBe('new');
    await rm(path.join(destination, DATABASE_NAME));
    expect(await migrateUserData(root, destination, log, backup)).toBe(false);
  } finally { await rm(root, { recursive: true, force: true }); }
});
it('does not publish a failed backup, retries safely and preserves existing settings', async () => {
  const root = await mkdtemp(path.resolve('tmp/rename-unit-')), source = path.join(root, LEGACY_DATA_NAMES[0]), destination = path.join(root, 'Shellfox');
  try {
    await mkdir(source); await mkdir(destination);
    await writeFile(path.join(source, DATABASE_NAME), 'old');
    await writeFile(path.join(source, 'settings.json'), 'old settings');
    await writeFile(path.join(destination, 'settings.json'), 'new settings');
    await expect(migrateUserData(root, destination, vi.fn(), async (_from, to) => { await writeFile(to, 'partial'); throw new Error('failed'); })).rejects.toThrow('failed');
    await expect(readFile(path.join(destination, DATABASE_NAME))).rejects.toMatchObject({ code: 'ENOENT' });
    expect(await readdir(path.join(destination, 'tmp'))).toEqual([]);
    expect(await migrateUserData(root, destination, vi.fn(), copyFile)).toBe(true);
    expect(await readFile(path.join(destination, 'settings.json'), 'utf8')).toBe('new settings');
  } finally { await rm(root, { recursive: true, force: true }); }
});
it('never replaces a destination database published while backup is in progress', async () => {
  const root = await mkdtemp(path.resolve('tmp/rename-race-'));
  const source = path.join(root, LEGACY_DATA_NAMES[0]), destination = path.join(root, 'Shellfox');
  try {
    await mkdir(source);
    await writeFile(path.join(source, DATABASE_NAME), 'legacy');
    const backup = async (from: string, staging: string) => {
      await copyFile(from, staging);
      await writeFile(path.join(destination, DATABASE_NAME), 'new concurrent data');
    };
    expect(await migrateUserData(root, destination, vi.fn(), backup)).toBe(false);
    expect(await readFile(path.join(destination, DATABASE_NAME), 'utf8')).toBe('new concurrent data');
    expect(await readdir(path.join(destination, 'tmp'))).toEqual([]);
    expect(await readFile(path.join(source, DATABASE_NAME), 'utf8')).toBe('legacy');
    await expect(readFile(path.join(destination, 'shellfox-upgrade.json'))).rejects.toMatchObject({ code: 'ENOENT' });
  } finally { await rm(root, { recursive: true, force: true }); }
});
it('does nothing when neither old installation has a database', async () => {
  const root = await mkdtemp(path.resolve('tmp/rename-unit-'));
  try { expect(await migrateUserData(root, path.join(root, 'Shellfox'))).toBe(false); }
  finally { await rm(root, { recursive: true, force: true }); }
});
