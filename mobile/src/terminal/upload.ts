import type { Result } from '../../../src/shared/contracts';
import { uploadResponseSchemas, type BeginTerminalUpload } from '../../../src/shared/terminal-upload-schemas';
import type { RemoteOperations } from '../client/operations';
import type { PickedFiles } from '../files';
import { RemoteClientError } from '../client/protocol';

export type TerminalUploadApi = Pick<RemoteOperations, 'beginTerminalUpload' | 'writeTerminalUpload' | 'finishTerminalUpload' | 'cancelTerminalUpload'>;
export type UploadIdentity = Pick<BeginTerminalUpload, 'tabId' | 'generation' | 'controlEpoch'>;
export interface UploadedFile { path: string; fileName: string; size: number }
export type UploadProgress = { phase: 'picking' } | { phase: 'uploading'; fileName: string; offset: number; size: number };
export class UploadCancelled extends Error {
  constructor() { super('Attachment cancelled.'); }
}
export function sameUploadIdentity(a: UploadIdentity | null, b: UploadIdentity): boolean {
  return a !== null && a.tabId === b.tabId && a.generation === b.generation && a.controlEpoch === b.controlEpoch;
}
function unwrap<T>(result: Result<T>): T {
  if (result.ok) return result.value;
  if ((result.error.code === 'VALIDATION' && /unknown remote operation/i.test(result.error.message)) || result.error.code === 'UNSUPPORTED') {
    throw new Error('Upgrade the Shellfox host to attach files.');
  }
  throw new RemoteClientError(result.error);
}

/** One sequential transfer. Never replay an upload request after an uncertain response. */
export class TerminalUpload {
  private cancelled = false;
  private uploadId: string | null = null;
  private finished = false;
  constructor(private readonly options: {
    api: TerminalUploadApi;
    files: PickedFiles;
    token: string;
    identity: UploadIdentity;
    isCurrent: () => boolean;
    onProgress: (value: UploadProgress) => void;
  }) {}
  private check(): void {
    if (this.cancelled || !this.options.isCurrent()) throw new UploadCancelled();
  }
  private progress(value: UploadProgress): void { this.check(); this.options.onProgress(value); }
  private async cancelRemote(): Promise<void> {
    if (!this.uploadId || this.finished) return;
    try { await this.options.api.cancelTerminalUpload({ ...this.options.identity, uploadId: this.uploadId }); }
    catch { /* Disconnect and ownership loss can make cleanup unreachable. The host expires partial files. */ }
  }
  cancel(): void {
    if (this.cancelled) return;
    this.cancelled = true;
    void this.options.files.cancelPick(this.options.token).catch(() => {});
    void this.cancelRemote();
  }
  async run(): Promise<UploadedFile | null> {
    const { api, files, identity, token } = this.options;
    let handle: string | null = null;
    try {
      this.progress({ phase: 'picking' });
      const file = await files.pick(token);
      handle = file?.handle ?? null;
      this.check();
      if (!file) return null;
      this.progress({ phase: 'uploading', fileName: file.fileName, offset: 0, size: file.size });
      const begun = unwrap(uploadResponseSchemas.beginTerminalUpload.parse(await api.beginTerminalUpload({ ...identity, fileName: file.fileName, size: file.size })));
      this.uploadId = begun.uploadId;
      this.check();
      for (let offset = 0; offset < file.size;) {
        const length = Math.min(begun.chunkBytes, file.size - offset);
        const chunk = await files.read(file.handle, offset, length);
        this.check();
        if (chunk.bytes !== length) throw new Error('The selected file changed.');
        const written = unwrap(uploadResponseSchemas.writeTerminalUpload.parse(await api.writeTerminalUpload({ ...identity, uploadId: begun.uploadId, offset, data: chunk.data })));
        this.check();
        if (written.offset !== offset + length) throw new Error('The host returned an unexpected file offset.');
        offset = written.offset;
        this.progress({ phase: 'uploading', fileName: file.fileName, offset, size: file.size });
      }
      this.check();
      const result = unwrap(uploadResponseSchemas.finishTerminalUpload.parse(await api.finishTerminalUpload({ ...identity, uploadId: begun.uploadId })));
      this.check();
      if (result.size !== file.size || /[\x00-\x1f\x7f]/.test(result.path)) throw new Error('The host returned an invalid attachment path.');
      this.finished = true;
      return result;
    } finally {
      // Do not let remote cleanup hold the local provider/cache open.
      if (handle) await files.release(handle).catch(() => {});
      if (!this.finished) await this.cancelRemote();
    }
  }
}
