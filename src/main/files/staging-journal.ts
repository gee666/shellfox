import path from 'node:path';
import { constants } from 'node:fs';
import { mkdir, open, lstat, stat, realpath, rename, unlink } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import { z } from 'zod';
import { filePathSchema } from '../../shared/file-schemas';
import type { FileInfo } from './transfer-io';

const MAX_BYTES = 4 * 1024 * 1024;
const identity = z.object({ dev: z.number().nonnegative(), ino: z.number().nonnegative() }).strict();
const entrySchema = z.object({ stage: filePathSchema, parent: filePathSchema, parentIdentity: identity, fileIdentity: identity.nullable() }).strict();
const journalSchema = z.object({ version: z.literal(1), entries: z.array(entrySchema).max(32) }).strict();
type Entry = z.infer<typeof entrySchema>;
const stageName = /^\.shellfox-transfer-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\.partial$/;
const sameIdentity = (info: FileInfo, expected: z.infer<typeof identity>) => info.dev === expected.dev && info.ino === expected.ino;
/** Only stages named in this host's private journal are eligible for recovery. Never scans user directories. */
export class StagingJournal {
  private readonly entries = new Map<string, Entry>();
  private queue: Promise<unknown> = Promise.resolve();
  private readonly file: string;
  constructor(private readonly userData: string) { this.file = path.join(userData, 'file-transfer-stages.json'); }
  async initialize(): Promise<void> {
    await mkdir(this.userData, { recursive: true, mode: 0o700 });
    let stored: z.infer<typeof journalSchema> = { version: 1, entries: [] };
    try {
      const before = await lstat(this.file);
      if (!before.isFile() || before.isSymbolicLink()) throw new Error('Invalid file transfer journal.');
      const handle = await open(this.file, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
      try {
        const info = await handle.stat(); if (!sameIdentity(info, before) || !info.isFile() || info.size > MAX_BYTES) throw new Error('Invalid file transfer journal.');
        const buffer = Buffer.alloc(MAX_BYTES + 1); let length = 0;
        while (length < buffer.length) { const read = await handle.read(buffer, length, buffer.length - length, length); if (!read.bytesRead) break; length += read.bytesRead; }
        if (length > MAX_BYTES) throw new Error('File transfer journal exceeds its limit.');
        stored = journalSchema.parse(JSON.parse(buffer.subarray(0, length).toString('utf8')));
      } finally { await handle.close(); }
    } catch (error) { if (!(error instanceof Error && 'code' in error && error.code === 'ENOENT')) throw error; }
    for (const entry of stored.entries) {
      if (!await this.recover(entry)) this.entries.set(entry.stage, entry);
    }
    await this.persist();
  }
  private async recover(entry: Entry): Promise<boolean> {
    if (!stageName.test(path.basename(entry.stage)) || path.dirname(entry.stage) !== entry.parent || !path.isAbsolute(entry.stage)) return false;
    try {
      if (await realpath(entry.parent) !== entry.parent || !sameIdentity(await stat(entry.parent), entry.parentIdentity)) return false;
      const info = await lstat(entry.stage);
      if (!info.isFile() || info.isSymbolicLink() || (process.getuid && info.uid !== process.getuid())) return false;
      if (entry.fileIdentity ? !sameIdentity(info, entry.fileIdentity) : info.size !== 0) return false;
      // A write-ahead intent without inode evidence can only cover the empty file
      // created before the second journal sync. No uploaded bytes exist yet.
      await unlink(entry.stage); return true;
    } catch (error) { return error instanceof Error && 'code' in error && error.code === 'ENOENT'; }
  }
  private async persist(): Promise<void> {
    const text = JSON.stringify({ version: 1, entries: [...this.entries.values()] });
    if (Buffer.byteLength(text) > MAX_BYTES) throw new Error('File transfer journal exceeds its limit.');
    const stage = this.file + '.' + randomUUID() + '.tmp';
    try {
      const handle = await open(stage, 'wx', 0o600);
      try { await handle.writeFile(text); await handle.sync(); } finally { await handle.close(); }
      await rename(stage, this.file);
      if (process.platform !== 'win32') {
        const directory = await open(this.userData, 'r'); try { await directory.sync(); } finally { await directory.close(); }
      }
    } finally { await unlink(stage).catch(() => {}); }
  }
  private mutate(action: () => void): Promise<void> {
    const operation = this.queue.then(async () => { action(); await this.persist(); });
    this.queue = operation.catch(() => {}); return operation;
  }
  intent(stage: string, parent: string, parentInfo: FileInfo): Promise<void> {
    return this.mutate(() => {
      if (this.entries.size >= 32 || !stageName.test(path.basename(stage)) || path.dirname(stage) !== parent) throw new Error('File transfer journal is full or invalid.');
      this.entries.set(stage, { stage, parent, parentIdentity: { dev: parentInfo.dev, ino: parentInfo.ino }, fileIdentity: null });
    });
  }
  identify(stage: string, info: FileInfo): Promise<void> {
    return this.mutate(() => { const entry = this.entries.get(stage); if (!entry) throw new Error('Missing transfer journal intent.'); entry.fileIdentity = { dev: info.dev, ino: info.ino }; });
  }
  forget(stage: string): Promise<void> { return this.mutate(() => { this.entries.delete(stage); }); }
  drain(): Promise<unknown> { return this.queue; }
}
