import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { link, lstat, open, realpath, rename, unlink, type FileHandle } from 'node:fs/promises';
import type { DesktopFilesApi, DesktopFileInput } from '../shared/desktop-file-contracts';
import { localFileNameSchema } from '../shared/desktop-file-schemas';
import { FILE_CHUNK_BYTES, FILE_TRANSFER_MAX_BYTES, FILE_TRANSFER_TTL_MS } from '../shared/file-transfer-schemas';
import { fileResult, openRegular, requireValid, sameFile, type FileInfo } from './files/transfer-io';
import { FileError } from './files/paths';

export interface DesktopFileDialogs {
  upload(): Promise<string | null>;
  download(name: string): Promise<string | null>;
}
interface Entry {
  id: string; file: FileHandle; offset: number; size: number; active: boolean;
  expires: number; pending?: Promise<unknown>;
}
interface Upload extends Entry { kind: 'upload'; info: FileInfo }
interface Download extends Entry { kind: 'download'; target: string; stage: string; expected: FileInfo | null; parent: FileInfo }
type Handle = Upload | Download;
const MAX_HANDLES = 8;
const existing = async (file: string): Promise<FileInfo | null> => {
  try { return await lstat(file); }
  catch (error) { if (error instanceof Error && 'code' in error && error.code === 'ENOENT') return null; throw error; }
};

/** One instance belongs to one renderer document. A reload gets a fresh instance. */
export class DesktopFileHandles implements DesktopFilesApi {
  private readonly entries = new Map<string, Handle>();
  private disposed = false;
  private choosing = false;
  private readonly timer: ReturnType<typeof setInterval>;
  constructor(private readonly dialogs: DesktopFileDialogs) {
    this.timer = setInterval(() => {
      for (const entry of this.entries.values()) if (entry.expires < Date.now()) void this.remove(entry);
    }, 30_000);
    this.timer.unref();
  }
  private valid(entry?: Handle): boolean { return !this.disposed && (!entry || entry.active && entry.expires >= Date.now()); }
  private async choose<T>(action: () => Promise<T>): Promise<T> {
    requireValid(() => this.valid());
    if (this.choosing || this.entries.size >= MAX_HANDLES) throw new FileError('VALIDATION', 'Finish or cancel the current local file transfer first.');
    this.choosing = true;
    try { return await action(); } finally { this.choosing = false; }
  }
  private find(id: string, kind: 'upload'): Upload;
  private find(id: string, kind: 'download'): Download;
  private find(id: string, kind: Handle['kind']): Handle {
    const entry = this.entries.get(id);
    if (!entry || entry.kind !== kind || !this.valid(entry)) throw new FileError('NOT_FOUND', 'Local file handle expired or belongs to another document.');
    if (entry.pending) throw new FileError('VALIDATION', 'Local file chunks must be processed sequentially.');
    entry.expires = Date.now() + FILE_TRANSFER_TTL_MS;
    return entry;
  }
  private async run<E extends Handle, T>(entry: E, action: (entry: E) => Promise<T>): Promise<T> {
    const pending = action(entry);
    entry.pending = pending;
    try { return await pending; } finally { if (entry.pending === pending) entry.pending = undefined; }
  }
  private async remove(entry: Handle): Promise<void> {
    entry.active = false;
    this.entries.delete(entry.id);
    await entry.pending?.catch(() => {});
    await entry.file.close().catch(() => {});
    if (entry.kind === 'download') await unlink(entry.stage).catch(() => {});
  }
  pickUpload(): ReturnType<DesktopFilesApi['pickUpload']> {
    return fileResult(() => this.choose(async () => {
      const selected = await this.dialogs.upload();
      requireValid(() => this.valid());
      if (!selected) return null;
      const name = path.basename(selected);
      if (!localFileNameSchema.safeParse(name).success) throw new FileError('UNSUPPORTED', 'The selected filename contains unsupported characters.');
      const canonical = await realpath(selected), file = await openRegular(canonical);
      try {
        const info = await file.stat();
        if (info.size > FILE_TRANSFER_MAX_BYTES) throw new FileError('UNSUPPORTED', 'File exceeds the local transfer size limit.');
        requireValid(() => this.valid());
        const id = randomUUID();
        this.entries.set(id, { id, kind: 'upload', file, info, size: info.size, offset: 0, active: true, expires: Date.now() + FILE_TRANSFER_TTL_MS });
        return { handle: id, name, size: info.size, chunkBytes: FILE_CHUNK_BYTES };
      } catch (error) { await file.close(); throw error; }
    }));
  }
  readUpload(input: DesktopFileInput<'readUpload'>): ReturnType<DesktopFilesApi['readUpload']> {
    return fileResult(() => this.run(this.find(input.handle, 'upload'), async entry => {
      if (input.offset !== entry.offset) throw new FileError('VALIDATION', 'Unexpected local file offset.');
      if (!sameFile(entry.info, await entry.file.stat())) throw new FileError('FILE_CONFLICT', 'Local file changed while uploading.');
      const bytes = Buffer.alloc(Math.min(FILE_CHUNK_BYTES, entry.size - entry.offset));
      let read = 0;
      while (read < bytes.length) {
        requireValid(() => this.valid(entry));
        const result = await entry.file.read(bytes, read, bytes.length - read, entry.offset + read);
        if (!result.bytesRead) throw new FileError('FILE_CONFLICT', 'Local file changed while uploading.');
        read += result.bytesRead;
      }
      requireValid(() => this.valid(entry));
      if (!sameFile(entry.info, await entry.file.stat())) throw new FileError('FILE_CONFLICT', 'Local file changed while uploading.');
      requireValid(() => this.valid(entry));
      entry.offset += read;
      return { offset: entry.offset, data: bytes.toString('base64'), eof: entry.offset === entry.size };
    }));
  }
  releaseUpload(input: DesktopFileInput<'releaseUpload'>): ReturnType<DesktopFilesApi['releaseUpload']> {
    return fileResult(async () => {
      const entry = this.entries.get(input.handle);
      if (entry?.kind === 'upload') await this.remove(entry);
      return { released: true as const };
    });
  }
  beginDownload(input: DesktopFileInput<'beginDownload'>): ReturnType<DesktopFilesApi['beginDownload']> {
    return fileResult(() => this.choose(async () => {
      const selected = await this.dialogs.download(input.name);
      requireValid(() => this.valid());
      if (!selected) return null;
      const target = path.join(await realpath(path.dirname(selected)), path.basename(selected));
      const expected = await existing(target), parent = await lstat(path.dirname(target));
      if (expected && (!expected.isFile() || expected.isSymbolicLink())) throw new FileError('ACCESS_DENIED', 'The download destination must be a regular file.');
      const id = randomUUID(), stage = path.join(path.dirname(target), `.shellfox-download-${id}`);
      const file = await open(stage, 'wx', 0o600);
      try {
        requireValid(() => this.valid());
        this.entries.set(id, { id, kind: 'download', file, target, stage, expected, parent, size: input.size, offset: 0, active: true, expires: Date.now() + FILE_TRANSFER_TTL_MS });
        return { handle: id };
      } catch (error) { await file.close(); await unlink(stage).catch(() => {}); throw error; }
    }));
  }
  writeDownload(input: DesktopFileInput<'writeDownload'>): ReturnType<DesktopFilesApi['writeDownload']> {
    return fileResult(() => this.run(this.find(input.handle, 'download'), async entry => {
      const bytes = Buffer.from(input.data, 'base64');
      if (input.offset !== entry.offset || !bytes.length || bytes.length > FILE_CHUNK_BYTES || entry.offset + bytes.length > entry.size || bytes.toString('base64') !== input.data) throw new FileError('VALIDATION', 'Invalid local download chunk or offset.');
      let written = 0;
      while (written < bytes.length) {
        requireValid(() => this.valid(entry));
        const result = await entry.file.write(bytes, written, bytes.length - written, entry.offset + written);
        if (!result.bytesWritten) throw new Error('The local download could not be written.');
        written += result.bytesWritten;
      }
      requireValid(() => this.valid(entry));
      entry.offset += written;
      return { offset: entry.offset };
    }));
  }
  finishDownload(input: DesktopFileInput<'finishDownload'>): ReturnType<DesktopFilesApi['finishDownload']> {
    return fileResult(() => this.run(this.find(input.handle, 'download'), async entry => {
      if (entry.offset !== entry.size) throw new FileError('VALIDATION', 'The local download is incomplete.');
      await entry.file.sync();
      if (entry.expected && process.platform !== 'win32') await entry.file.chmod(Number(entry.expected.mode) & 0o777);
      const written = await entry.file.stat();
      await entry.file.close();
      const staged = await existing(entry.stage), parent = await lstat(path.dirname(entry.target));
      if (!staged || !staged.isFile() || !sameFile(written, staged) || parent.dev !== entry.parent.dev || parent.ino !== entry.parent.ino || !parent.isDirectory()) throw new FileError('FILE_CONFLICT', 'The local download stage or destination folder changed.');
      const current = await existing(entry.target);
      if (entry.expected ? !current || !sameFile(entry.expected, current) : current !== null) throw new FileError('FILE_CONFLICT', 'The local destination changed after choosing it. Download again to another file.');
      requireValid(() => this.valid(entry));
      if (entry.expected) await rename(entry.stage, entry.target);
      else { await link(entry.stage, entry.target); await unlink(entry.stage); }
      entry.active = false; this.entries.delete(entry.id);
      return { saved: true as const };
    }));
  }
  cancelDownload(input: DesktopFileInput<'cancelDownload'>): ReturnType<DesktopFilesApi['cancelDownload']> {
    return fileResult(async () => {
      const entry = this.entries.get(input.handle);
      if (entry?.kind === 'download') await this.remove(entry);
      return { cancelled: true as const };
    });
  }
  async dispose(): Promise<void> {
    this.disposed = true; clearInterval(this.timer);
    await Promise.allSettled([...this.entries.values()].map(entry => this.remove(entry)));
  }
}
