import type { Result } from '../../src/shared/contracts';
import type { FileLocation } from '../../src/shared/file-contracts';
import type { FileTransferApi } from '../../src/shared/file-transfer-contracts';
import { FILE_TRANSFER_MAX_BYTES, fileTransferValueSchemas } from '../../src/shared/file-transfer-schemas';
import type { PickedFiles } from './files';
import type { Downloads, SavedDownload } from './downloads';

export type FileTransferProgress = { phase: 'picking' } | { phase: 'exporting' } | { phase: 'uploading' | 'downloading'; offset: number; size: number; fileName: string };
export class FileTransferCancelled extends Error { constructor() { super('File transfer cancelled.'); } }
function unwrap<T>(result: Result<T>): T { if (!result.ok) throw new Error(result.error.message); return result.value; }
export function remoteFileName(path: string): string { return path.split(/[\\/]/).at(-1) || 'download'; }
function uploadPath(folder: string, name: string): string {
  if (!name || name === '.' || name === '..' || name.length > 255 || /[\x00-\x1f\x7f/\\]/.test(name)) throw new Error('The selected file has an invalid name.');
  return `${folder.replace(/[\\/]+$/, '')}/${name}`;
}
export function streamingFiles(api: Partial<FileTransferApi>): FileTransferApi | null {
  const { beginRead, readChunk, beginWrite, writeChunk, finishWrite, cancelTransfer } = api;
  return beginRead && readChunk && beginWrite && writeChunk && finishWrite && cancelTransfer
    ? { beginRead: input => beginRead.call(api, input), readChunk: input => readChunk.call(api, input), beginWrite: input => beginWrite.call(api, input),
      writeChunk: input => writeChunk.call(api, input), finishWrite: input => finishWrite.call(api, input), cancelTransfer: input => cancelTransfer.call(api, input) } : null;
}

/** Exactly one sequential transfer. Uncertain writes are never replayed. */
export class MobileFileTransfer {
  private cancelled = false;
  private transferId: string | null = null;
  private nativeHandle: string | null = null;
  constructor(private readonly options: {
    api: FileTransferApi;
    context: Omit<FileLocation, 'path'>;
    files: PickedFiles;
    downloads: Downloads;
    token: string;
    current: () => boolean;
    progress: (progress: FileTransferProgress) => void;
  }) {}
  private check() { if (this.cancelled || !this.options.current()) throw new FileTransferCancelled(); }
  private progress(value: FileTransferProgress) { this.check(); this.options.progress(value); }
  cancel() {
    if (this.cancelled) return;
    this.cancelled = true;
    void this.options.files.cancelPick(this.options.token).catch(() => {});
    void this.options.downloads.cancel(this.nativeHandle ?? this.options.token).catch(() => {});
    void this.cancelRemote();
  }
  private async cancelRemote() {
    if (this.transferId) await this.options.api.cancelTransfer({ transferId: this.transferId }).catch(() => {});
  }
  async upload(folder: string): Promise<string | null> {
    let handle: string | null = null;
    try {
      this.progress({ phase: 'picking' });
      const file = await this.options.files.pick(this.options.token, FILE_TRANSFER_MAX_BYTES);
      handle = file?.handle ?? null; this.check();
      if (!file) return null;
      const path = uploadPath(folder, file.fileName);
      const begin = fileTransferValueSchemas.beginFileWrite.parse(unwrap(await this.options.api.beginWrite({ ...this.options.context, path, size: file.size, mode: 'create', content: 'file' })));
      this.transferId = begin.transferId; this.check();
      let offset = 0;
      this.progress({ phase: 'uploading', offset, size: file.size, fileName: file.fileName });
      while (offset < file.size) {
        const length = Math.min(begin.chunkBytes, file.size - offset);
        const chunk = await this.options.files.read(file.handle, offset, length); this.check();
        if (chunk.bytes !== length) throw new Error('The selected file changed.');
        const written = fileTransferValueSchemas.writeFileChunk.parse(unwrap(await this.options.api.writeChunk({ transferId: begin.transferId, offset, data: chunk.data })));
        this.check();
        if (written.offset !== offset + length) throw new Error('The host returned an unexpected upload offset.');
        offset = written.offset;
        this.progress({ phase: 'uploading', offset, size: file.size, fileName: file.fileName });
      }
      const result = fileTransferValueSchemas.finishFileWrite.parse(unwrap(await this.options.api.finishWrite({ transferId: begin.transferId })));
      if (result.size !== file.size) throw new Error('The host saved a different file size.');
      return result.path;
    } finally {
      if (handle) await this.options.files.release(handle).catch(() => {});
      await this.cancelRemote();
    }
  }
  async download(path: string): Promise<SavedDownload | null> {
    let complete = false;
    try {
      this.progress({ phase: 'downloading', offset: 0, size: 0, fileName: remoteFileName(path) });
      const begin = fileTransferValueSchemas.beginFileRead.parse(unwrap(await this.options.api.beginRead({ ...this.options.context, path })));
      this.transferId = begin.transferId; this.check();
      const fileName = remoteFileName(begin.path);
      const mimeType = begin.mimeType.split(';')[0].trim();
      const type = /^[A-Za-z0-9.+-]+\/[A-Za-z0-9.+-]+$/.test(mimeType) ? mimeType : 'application/octet-stream';
      const local = await this.options.downloads.begin(this.options.token, fileName, begin.size, type);
      this.nativeHandle = local?.handle ?? null; this.check();
      if (!local) return null;
      let offset = 0;
      this.progress({ phase: 'downloading', offset, size: begin.size, fileName });
      do {
        const chunk = fileTransferValueSchemas.readFileChunk.parse(unwrap(await this.options.api.readChunk({ transferId: begin.transferId, offset })));
        this.check();
        const padding = chunk.data.endsWith('==') ? 2 : chunk.data.endsWith('=') ? 1 : 0;
        const bytes = chunk.data.length * 3 / 4 - padding;
        if (offset + bytes !== chunk.offset || chunk.offset > begin.size || bytes > begin.chunkBytes || chunk.eof !== (chunk.offset === begin.size) || !bytes && !chunk.eof) throw new Error('The host returned an invalid download chunk.');
        if (bytes) {
          const written = await this.options.downloads.write(local.handle, offset, chunk.data); this.check();
          if (written.offset !== chunk.offset) throw new Error('The device returned an unexpected download offset.');
        }
        offset = chunk.offset;
        this.progress({ phase: 'downloading', offset, size: begin.size, fileName });
        if (chunk.eof) break;
      } while (offset < begin.size);
      this.progress({ phase: 'exporting' });
      const saved = await this.options.downloads.finish(local.handle);
      complete = saved !== null;
      if (!complete) this.check();
      return saved;
    } finally {
      if (!complete && this.nativeHandle) await this.options.downloads.cancel(this.nativeHandle).catch(() => {});
      await this.cancelRemote();
    }
  }
}
