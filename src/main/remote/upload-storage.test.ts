import { mkdir, mkdtemp, readdir, readFile, rm, symlink, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { afterEach, expect, it } from 'vitest';
import { UploadStorage, sanitizeUploadName, type StoredUpload } from './upload-storage';

const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => { for (const fn of cleanups.splice(0).reverse()) await fn(); });
async function fixture() {
  await mkdir('tmp', { recursive: true }); const directory = await mkdtemp(path.resolve('tmp/upload-storage-'));
  cleanups.push(() => rm(directory, { recursive: true, force: true })); return { directory, root: path.join(directory, 'uploads') };
}
it('sanitizes traversal, reserved devices, alternate streams and invisible/command text', () => {
  for (const [name, expected] of [['../../foo.txt', 'foo.txt'], ['C:\\evil\\NUL.jpg', 'file_NUL.jpg'], ['. .', 'file'], ['a.txt:evil', 'a.txt_evil'], ['foo.  ', 'foo'], ['\u202eevil\u0000.png', '_evil_.png'], ['$(touch x);.sh', '__touch x__.sh']]) expect(sanitizeUploadName(name)).toBe(expected);
});
it('fails explicit invalid configuration instead of falling back to another directory', () => {
  expect(() => new UploadStorage({ root: 'relative' })).toThrow();
  for (const maxBytes of [0, -1, 1.5, NaN, 1024 ** 3 + 1]) expect(() => new UploadStorage({ maxBytes })).toThrow();
});
it('bounds reserved disk space and counts completed files across a restart', async () => {
  const f = await fixture(), storage = new UploadStorage({ root: f.root });
  const reserved: StoredUpload[] = [];
  try {
    for (let i = 0; i < 8; i++) reserved.push(await storage.create('big.bin', 64 * 1024 * 1024));
    await expect(storage.create('over', 1)).rejects.toThrow('storage limit');
  } finally { for (const file of reserved) { await file.handle.close(); await storage.remove(file.id); } }
  const saved = await storage.create('kept', 0); await storage.save(saved, () => true);
  const restarted = new UploadStorage({ root: f.root }); await restarted.check();
  expect(await readFile(saved.destination)).toEqual(Buffer.alloc(0));
  expect(await readdir(f.root)).toEqual([saved.id]);
});
it('expires saved files and removes orphaned partial files left by a crashed host', async () => {
  const f = await fixture(); let now = Date.now();
  const storage = new UploadStorage({ root: f.root, now: () => now });
  const saved = await storage.create('kept', 0); await storage.save(saved, () => true);
  const abandoned = await storage.create('partial', 1); await abandoned.handle.close();
  const restarted = new UploadStorage({ root: f.root }); await restarted.check();
  expect(await readdir(f.root)).toEqual([saved.id]);
  now += 24 * 60 * 60_000 + 1; await storage.sweep();
  expect(await readdir(f.root)).toEqual([]);
});
it('initializes retention on the first periodic sweep after restart without another upload', async () => {
  const f = await fixture(), original = new UploadStorage({ root: f.root });
  const saved = await original.create('expired', 0); await original.save(saved, () => true);
  const abandoned = await original.create('abandoned', 1); await abandoned.handle.close();
  const restarted = new UploadStorage({ root: f.root, now: () => Date.now() + 24 * 60 * 60_000 + 1 });
  await Promise.all([restarted.sweep(), restarted.sweep()]);
  expect(await readdir(f.root)).toEqual([]);
  await restarted.sweep(); expect(await readdir(f.root)).toEqual([]);
});
it('refuses symlink roots and does not follow a destination symlink on publication', async () => {
  const f = await fixture(), outside = path.join(f.directory, 'outside'); await mkdir(outside);
  try { await symlink(outside, f.root, process.platform === 'win32' ? 'junction' : 'dir'); }
  catch (error) { if (process.platform === 'win32' && error instanceof Error && 'code' in error && error.code === 'EPERM') return; throw error; }
  await expect(new UploadStorage({ root: f.root }).check()).rejects.toThrow();
  await rm(f.root); const storage = new UploadStorage({ root: f.root }), file = await storage.create('x', 0);
  const target = path.join(outside, 'target'); await writeFile(target, 'original');
  try { await symlink(target, file.destination); }
  catch (error) { await file.handle.close(); if (process.platform === 'win32' && error instanceof Error && 'code' in error && error.code === 'EPERM') return; throw error; }
  await expect(storage.save(file, () => true)).rejects.toThrow();
  expect(await readFile(target, 'utf8')).toBe('original');
});
