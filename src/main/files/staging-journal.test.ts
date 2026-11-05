import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { mkdir, mkdtemp, open, stat, writeFile, readFile, rm, access } from 'node:fs/promises';
import { afterEach, expect, it } from 'vitest';
import { StagingJournal } from './staging-journal';
import { FileTransfers } from './transfers';
import { responseSchemas } from '../../shared/schemas';
const cleanup: string[] = [];
afterEach(async () => { for (const directory of cleanup.splice(0)) await rm(directory, { recursive: true, force: true }); });
async function fixture() {
  await mkdir('tmp', { recursive: true }); const root = await mkdtemp(path.resolve('tmp/file-journal-')); cleanup.push(root);
  const data = path.join(root, 'host'), parent = path.join(root, 'destination'); await mkdir(parent);
  const journal = new StagingJournal(data); await journal.initialize();
  return { root, data, parent, journal, name: () => path.join(parent, `.shellfox-transfer-${randomUUID()}.partial`) };
}
it('recovers only known interrupted stages after restart without deleting completed destination files', async () => {
  const f = await fixture(), stage = f.name(), destination = path.join(f.parent, 'user-file.txt');
  await writeFile(destination, 'permanent');
  await f.journal.intent(stage, f.parent, await stat(f.parent));
  const handle = await open(stage, 'wx', 0o600); await f.journal.identify(stage, await handle.stat()); await handle.writeFile('incomplete bytes'); await handle.close();
  await new StagingJournal(f.data).initialize();
  await expect(access(stage)).rejects.toMatchObject({ code: 'ENOENT' }); expect(await readFile(destination, 'utf8')).toBe('permanent');
  expect(JSON.parse(await readFile(path.join(f.data, 'file-transfer-stages.json'), 'utf8'))).toEqual({ version: 1, entries: [] });
});
it('recovers empty write-ahead intents but refuses to delete nonempty files without inode evidence', async () => {
  const f = await fixture(), empty = f.name(), uncertain = f.name();
  await f.journal.intent(empty, f.parent, await stat(f.parent)); await writeFile(empty, '', { mode: 0o600, flag: 'wx' });
  await f.journal.intent(uncertain, f.parent, await stat(f.parent)); await writeFile(uncertain, 'not authenticated', { mode: 0o600, flag: 'wx' });
  await new StagingJournal(f.data).initialize();
  await expect(access(empty)).rejects.toMatchObject({ code: 'ENOENT' }); expect(await readFile(uncertain, 'utf8')).toBe('not authenticated');
});
it('refuses unrelated names, mismatching file identities and changed canonical parents', async () => {
  const f = await fixture(), ordinary = path.join(f.parent, 'ordinary.txt'); await writeFile(ordinary, 'do not delete');
  await expect(f.journal.intent(ordinary, f.parent, await stat(f.parent))).rejects.toThrow();
  const stage = f.name(); await f.journal.intent(stage, f.parent, await stat(f.parent)); await writeFile(stage, 'keep');
  await f.journal.identify(stage, await stat(ordinary));
  await new StagingJournal(f.data).initialize();
  expect(await readFile(stage, 'utf8')).toBe('keep'); expect(await readFile(ordinary, 'utf8')).toBe('do not delete');
});
it('journals production transfers before accepting bytes and clears records after cancellation', async () => {
  const f = await fixture(), transfers = new FileTransfers({ session: () => undefined, tab: () => undefined }); await transfers.initialize(f.data);
  try {
    const begin = responseSchemas.beginFileWrite.parse(await transfers.request({ id: 'view', valid: () => true }, 'beginFileWrite', { path: path.join(f.parent, 'upload.bin'), mode: 'create', size: 1, content: 'file' }));
    const parsed = JSON.parse(await readFile(path.join(f.data, 'file-transfer-stages.json'), 'utf8'));
    expect(parsed.entries).toHaveLength(1); expect(parsed.entries[0].fileIdentity).toEqual(expect.objectContaining({ dev: expect.any(Number), ino: expect.any(Number) }));
    if (!begin.ok) throw new Error('No transfer');
    await transfers.request({ id: 'view', valid: () => true }, 'cancelFileTransfer', { transferId: begin.value.transferId });
    expect(JSON.parse(await readFile(path.join(f.data, 'file-transfer-stages.json'), 'utf8')).entries).toEqual([]);
  } finally { await transfers.dispose(); }
});
