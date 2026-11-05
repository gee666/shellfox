import path from 'node:path';
import { constants } from 'node:fs';
import { open, realpath, stat, lstat, link, rename, unlink } from 'node:fs/promises';
import type { FileHandle } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import type { FileTransferInput, FileTransferValue } from '../../shared/file-transfer-contracts';
import { FILE_CHUNK_BYTES, FILE_TRANSFER_DEFAULT_BYTES, FILE_TRANSFER_MAX_BYTES, FILE_TRANSFER_TTL_MS, fileTransferRequestSchemas, type FileTransferMethod } from '../../shared/file-transfer-schemas';
import { FILE_IMAGE_WRITE_BYTES, FILE_TEXT_BYTES } from '../../shared/file-schemas';
import { resolveFileTarget, FileError, type FileSessionContext, type FileTarget } from './paths';
import { StagingJournal } from './staging-journal';
import { mimeType } from './content';
import { fileResult, openRegular, inspectFile, assetTarget, requireValid, sameFile, type FileInfo } from './transfer-io';
export interface FileActor { id: string; valid: () => boolean }
interface Transfer {
  id: string; owner: string; handle: FileHandle; file: string; size: number; offset: number; touched: number; busy: boolean; cancelled: boolean; valid: () => boolean;
  info: FileInfo; revision: string; target: FileTarget;
  write?: { input: FileTransferInput<'beginFileWrite'>; stage: string; parent: string; parentInfo: FileInfo; existing: FileInfo | null };
}
export interface FileTransferOptions { maxBytes?: number; now?: () => number }
export class FileTransfers {
  private readonly entries = new Map<string, Transfer>();
  private readonly starting = new Map<string, number>();
  private readonly epochs = new Map<string, number>();
  private readonly inflight = new Map<string, number>();
  private readonly operations = new Set<Promise<unknown>>();
  private readonly cleanup = new Set<Promise<unknown>>();
  private timer?: ReturnType<typeof setInterval>;
  private disposed = false;
  private journal?: StagingJournal;
  private ready: Promise<void> = Promise.resolve();
  private reservedBytes = 0;
  readonly maxBytes: number;
  private readonly now: () => number;
  constructor(private readonly context: FileSessionContext, options: FileTransferOptions = {}) {
    this.maxBytes = options.maxBytes ?? Number(process.env.SHELLFOX_FILE_TRANSFER_MAX_BYTES ?? FILE_TRANSFER_DEFAULT_BYTES);
    if (!Number.isSafeInteger(this.maxBytes) || this.maxBytes < 1 || this.maxBytes > FILE_TRANSFER_MAX_BYTES) throw new Error('Invalid SHELLFOX_FILE_TRANSFER_MAX_BYTES.');
    this.now = options.now ?? Date.now;
  }
  initialize(userData: string): Promise<void> {
    if (this.journal) return this.ready;
    this.journal = new StagingJournal(userData);
    this.ready = this.journal.initialize(); return this.ready;
  }
  private track(task: Promise<unknown>): void { this.cleanup.add(task); void task.finally(() => this.cleanup.delete(task)).catch(() => {}); }
  private async remove(entry: Transfer): Promise<void> {
    entry.cancelled = true;
    await entry.handle.close().catch(() => {});
    if (entry.write) {
      try { await unlink(entry.write.stage); await this.journal?.forget(entry.write.stage); }
      catch (error) { if (error instanceof Error && 'code' in error && error.code === 'ENOENT') await this.journal?.forget(entry.write.stage); }
    }
    this.entries.delete(entry.id); this.releaseEpoch(entry.owner);
  }
  private releaseEpoch(owner: string): void {
    if (!this.inflight.has(owner) && !this.starting.has(owner) && ![...this.entries.values()].some(entry => entry.owner === owner)) this.epochs.delete(owner);
  }
  cancelActor(owner: string): void {
    this.epochs.set(owner, (this.epochs.get(owner) ?? 0) + 1);
    for (const entry of this.entries.values()) if (entry.owner === owner) { entry.cancelled = true; if (!entry.busy) this.track(this.remove(entry)); }
    this.releaseEpoch(owner);
  }
  async sweep(): Promise<void> {
    for (const entry of this.entries.values()) if (this.now() - entry.touched > FILE_TRANSFER_TTL_MS || !entry.valid()) { entry.cancelled = true; if (!entry.busy) await this.remove(entry); }
  }
  request(actor: FileActor, method: FileTransferMethod, raw: unknown): Promise<unknown> {
    this.inflight.set(actor.id, (this.inflight.get(actor.id) ?? 0) + 1);
    const epoch = this.epochs.get(actor.id) ?? 0;
    const scoped = { id: actor.id, valid: () => !this.disposed && (this.epochs.get(actor.id) ?? 0) === epoch && actor.valid() };
    const task = fileResult(() => this.perform(scoped, method, raw)); this.operations.add(task);
    void task.finally(() => {
      this.operations.delete(task);
      const count = (this.inflight.get(actor.id) ?? 1) - 1; if (count) this.inflight.set(actor.id, count); else this.inflight.delete(actor.id);
      this.releaseEpoch(actor.id);
    }).catch(() => {}); return task;
  }
  private async perform(actor: FileActor, method: FileTransferMethod, raw: unknown): Promise<unknown> {
    await this.ready;
    const parsed = fileTransferRequestSchemas[method].safeParse(raw);
    if (!parsed.success) {
      if (raw && typeof raw === 'object' && 'transferId' in raw && typeof raw.transferId === 'string') {
        const entry = this.entries.get(raw.transferId);
        if (entry?.owner === actor.id) { entry.cancelled = true; if (!entry.busy) await this.remove(entry); }
      }
      throw new FileError('VALIDATION', 'Invalid file transfer request.');
    }
    if (this.disposed) throw new FileError('ACCESS_DENIED', 'File service stopped.');
    if (method === 'cancelFileTransfer') {
      const { transferId } = fileTransferRequestSchemas.cancelFileTransfer.parse(raw), entry = this.entries.get(transferId);
      if (!entry || entry.owner !== actor.id) throw new FileError('NOT_FOUND', 'Transfer not found for this connection.');
      entry.cancelled = true; if (!entry.busy) await this.remove(entry);
      return { cancelled: true };
    }
    requireValid(actor.valid);
    if (method === 'beginFileRead' || method === 'beginFileWrite') {
      await this.sweep();
      const pending = [...this.starting.values()].reduce((a, b) => a + b, 0);
      if ([...this.entries.values()].filter(e => e.owner === actor.id).length + (this.starting.get(actor.id) ?? 0) >= 2 || this.entries.size + pending >= 8) throw new FileError('VALIDATION', 'Too many file transfers. Finish or cancel one first.');
      const reserved = method === 'beginFileWrite' ? fileTransferRequestSchemas.beginFileWrite.parse(raw).size : 0;
      if (this.reservedBytes + [...this.entries.values()].reduce((sum, e) => sum + (e.write ? e.size : 0), 0) + reserved > this.maxBytes * 2) throw new FileError('VALIDATION', 'Host staging quota reached.');
      this.reservedBytes += reserved;
      this.starting.set(actor.id, (this.starting.get(actor.id) ?? 0) + 1);
      if (!this.timer) { this.timer = setInterval(() => { void this.sweep().catch(() => {}); }, 30_000); this.timer.unref(); }
      try {
        return method === 'beginFileRead' ? await this.beginRead(actor, fileTransferRequestSchemas.beginFileRead.parse(raw)) : await this.beginWrite(actor, fileTransferRequestSchemas.beginFileWrite.parse(raw));
      } finally { this.reservedBytes -= reserved; const count = (this.starting.get(actor.id) ?? 1) - 1; if (count) this.starting.set(actor.id, count); else this.starting.delete(actor.id); }
    }
    const input = fileTransferRequestSchemas[method].parse(raw), entry = this.entries.get('transferId' in input ? input.transferId : '');
    if (!entry || entry.owner !== actor.id) throw new FileError('NOT_FOUND', 'Transfer not found for this connection.');
    if (entry.busy) throw new FileError('VALIDATION', 'A transfer operation is already in progress.');
    if (this.now() - entry.touched > FILE_TRANSFER_TTL_MS) { await this.remove(entry); throw new FileError('NOT_FOUND', 'Transfer expired.'); }
    const valid = () => !this.disposed && !entry.cancelled && actor.valid() && entry.valid();
    entry.busy = true; entry.touched = this.now();
    try {
      requireValid(valid);
      if (method === 'readFileChunk') {
        if (entry.write) throw new FileError('VALIDATION', 'This is a write transfer.');
        const chunk = fileTransferRequestSchemas.readFileChunk.parse(raw);
        if (chunk.offset !== entry.offset) throw new FileError('VALIDATION', 'Unexpected download offset.');
        if (!sameFile(entry.info, await entry.handle.stat())) throw new FileError('FILE_CONFLICT', 'File changed during download.');
        const bytes = Buffer.alloc(Math.min(FILE_CHUNK_BYTES, entry.size - entry.offset));
        let received = 0;
        while (received < bytes.length) {
          requireValid(valid); const read = await entry.handle.read(bytes, received, bytes.length - received, entry.offset + received);
          if (!read.bytesRead) throw new FileError('FILE_CONFLICT', 'File changed during download.'); received += read.bytesRead;
        }
        if (!sameFile(entry.info, await entry.handle.stat())) throw new FileError('FILE_CONFLICT', 'File changed during download.');
        requireValid(valid); entry.offset += received;
        return { offset: entry.offset, data: bytes.toString('base64'), eof: entry.offset === entry.size };
      }
      if (!entry.write) throw new FileError('VALIDATION', 'This is a read transfer.');
      if (method === 'writeFileChunk') {
        const chunk = fileTransferRequestSchemas.writeFileChunk.parse(raw), bytes = Buffer.from(chunk.data, 'base64');
        if (chunk.offset !== entry.offset || bytes.length + entry.offset > entry.size) throw new FileError('VALIDATION', 'Upload offset or size does not match.');
        await this.verifyStage(entry);
        let written = 0;
        while (written < bytes.length) { requireValid(valid); const result = await entry.handle.write(bytes, written, bytes.length - written, entry.offset + written); if (!result.bytesWritten) throw new Error('Empty write'); written += result.bytesWritten; }
        requireValid(valid); entry.offset += written; return { offset: entry.offset };
      }
      return await this.finishWrite(entry, valid);
    } catch (error) { entry.cancelled = true; throw error; }
    finally { entry.busy = false; if (entry.cancelled) await this.remove(entry); }
  }
  private async beginRead(actor: FileActor, input: FileTransferInput<'beginFileRead'>): Promise<FileTransferValue<'beginFileRead'>> {
    const { assetPath, ...location } = input, target = await resolveFileTarget(this.context, location);
    const asset = assetPath === undefined ? null : await assetTarget(target.native, assetPath);
    const file = asset?.file ?? await realpath(target.native), handle = await openRegular(file);
    try {
      const info = await handle.stat(); if (info.size > this.maxBytes) throw new FileError('UNSUPPORTED', `File exceeds the host transfer limit of ${this.maxBytes} bytes.`);
      const metadata = await inspectFile(handle, file, actor.valid); requireValid(actor.valid);
      const id = randomUUID();
      this.entries.set(id, { id, owner: actor.id, handle, info, file, size: info.size, offset: 0, touched: this.now(), busy: false, cancelled: false, valid: actor.valid, revision: metadata.revision, target });
      return { ...metadata, ...(asset ? { mimeType: asset.mimeType } : {}), transferId: id, path: target.display(file), size: info.size, chunkBytes: FILE_CHUNK_BYTES };
    } catch (error) { await handle.close(); throw error; }
  }
  private async beginWrite(actor: FileActor, input: FileTransferInput<'beginFileWrite'>): Promise<FileTransferValue<'beginFileWrite'>> {
    if (input.size > this.maxBytes || (input.content === 'text' && input.size > FILE_TEXT_BYTES) || (input.content === 'image' && input.size > FILE_IMAGE_WRITE_BYTES)) throw new FileError('VALIDATION', 'File exceeds the configured transfer or editor limit.');
    const { size: _size, mode: _mode, revision: _revision, content: _content, mimeType: _mime, ...location } = input;
    const target = await resolveFileTarget(this.context, location), parent = await realpath(path.dirname(target.native)), parentInfo = await stat(parent);
    if (!parentInfo.isDirectory()) throw new FileError('NOT_FOUND', 'Destination directory not found.');
    let file = path.join(parent, path.basename(target.native)), existing: FileInfo | null = null;
    if (input.mode === 'create') {
      try { await lstat(file); throw new FileError('FILE_CONFLICT', 'A file already exists at this destination.'); } catch (error) { if (!(error instanceof Error && 'code' in error && error.code === 'ENOENT')) throw error; }
    } else {
      file = await realpath(target.native); if (path.dirname(file) !== parent) throw new FileError('ACCESS_DENIED', 'Replacement must stay in its selected directory.');
      const handle = await openRegular(file);
      try {
        existing = await handle.stat();
        if (existing.size > this.maxBytes) throw new FileError('UNSUPPORTED', 'Existing file exceeds the host transfer limit.');
        const previous = await inspectFile(handle, file, actor.valid);
        if (previous.revision !== input.revision) throw new FileError('FILE_CONFLICT', 'File changed on disk. Reload before saving.');
        if (input.content === 'image' && (previous.kind !== 'image' || previous.mimeType !== input.mimeType)) throw new FileError('UNSUPPORTED', 'Image editing requires an existing matching image.');
      } finally { await handle.close(); }
    }
    const id = randomUUID(), stage = path.join(parent, `.shellfox-transfer-${id}.partial`);
    await this.journal?.intent(stage, parent, parentInfo);
    let handle: FileHandle;
    try { handle = await open(stage, constants.O_CREAT | constants.O_EXCL | constants.O_RDWR | (constants.O_NOFOLLOW ?? 0), 0o600); }
    catch (error) { await this.journal?.forget(stage); throw error; }
    try {
      await this.journal?.identify(stage, await handle.stat());
      requireValid(actor.valid);
      this.entries.set(id, { id, owner: actor.id, handle, info: await handle.stat(), file, size: input.size, offset: 0, touched: this.now(), busy: false, cancelled: false, valid: actor.valid, revision: input.revision ?? '', target, write: { input, stage, parent, parentInfo, existing } });
      return { transferId: id, chunkBytes: FILE_CHUNK_BYTES };
    } catch (error) { await handle.close(); await unlink(stage).catch(() => {}); await this.journal?.forget(stage); throw error; }
  }
  private async verifyStage(entry: Transfer): Promise<void> {
    const write = entry.write; if (!write) throw new Error('Not a write transfer');
    const parent = await stat(write.parent), staged = await lstat(write.stage), opened = await entry.handle.stat();
    if (await realpath(path.dirname(entry.target.native)) !== write.parent || parent.dev !== write.parentInfo.dev || parent.ino !== write.parentInfo.ino || !staged.isFile() || staged.isSymbolicLink() || staged.dev !== opened.dev || staged.ino !== opened.ino) throw new FileError('ACCESS_DENIED', 'Transfer directory or staging file changed.');
  }
  private async finishWrite(entry: Transfer, valid: () => boolean): Promise<FileTransferValue<'finishFileWrite'>> {
    const write = entry.write; if (!write) throw new Error('Not a write transfer');
    if (entry.offset !== entry.size) throw new FileError('VALIDATION', 'Upload is incomplete.');
    await this.verifyStage(entry);
    const metadata = await inspectFile(entry.handle, entry.file, valid);
    if (write.input.content === 'text' && metadata.kind !== 'text') throw new FileError('UNSUPPORTED', 'Only UTF-8 text without NUL bytes can be saved by the editor.');
    if (write.input.content === 'image' && (metadata.kind !== 'image' || metadata.mimeType !== write.input.mimeType || mimeType(entry.file) !== write.input.mimeType)) throw new FileError('VALIDATION', 'Image MIME type, extension and magic header must match.');
    if (write.existing) {
      if (await realpath(entry.target.native) !== entry.file) throw new FileError('FILE_CONFLICT', 'Destination changed.');
      const current = await openRegular(entry.file);
      try { if (!sameFile(write.existing, await current.stat()) || (await inspectFile(current, entry.file, valid)).revision !== entry.revision) throw new FileError('FILE_CONFLICT', 'File changed on disk. Reload before saving.'); } finally { await current.close(); }
      if (process.platform !== 'win32') await entry.handle.chmod(write.existing.mode & 0o777);
    }
    await entry.handle.sync(); await this.verifyStage(entry); await entry.handle.close(); requireValid(valid);
    if (write.input.mode === 'create') { await link(write.stage, entry.file); await unlink(write.stage); }
    else await rename(write.stage, entry.file);
    this.entries.delete(entry.id); await this.journal?.forget(write.stage);
    return { path: entry.target.display(entry.file), size: entry.size, revision: metadata.revision };
  }
  async dispose(): Promise<void> {
    this.disposed = true; if (this.timer) clearInterval(this.timer);
    for (const entry of this.entries.values()) { entry.cancelled = true; if (!entry.busy) this.track(this.remove(entry)); }
    await Promise.allSettled(this.operations); await Promise.allSettled(this.cleanup); await this.journal?.drain();
  }
}
