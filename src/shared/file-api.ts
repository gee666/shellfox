import { failure, success, type Result } from './contracts';
import type { FileApi, FileRead, FileWriteResult, FileLocation, FileAsset } from './file-contracts';
import type { FileTransferApi, FileTransferInput } from './file-transfer-contracts';
import { decodeFileUtf8, isWellFormedFileText } from './file-utf8';
import { FILE_CHUNK_BYTES } from './file-transfer-schemas';
import { FILE_IMAGE_BYTES, FILE_TEXT_BYTES, fileReadSchema, fileAssetSchema, fileLocationSchema, fileEditorWriteSchema, fileEditorImageWriteSchema, fileAssetRequestSchema } from './file-schemas';

export interface FileWire extends FileTransferApi { list: FileApi['list'] }
export function encodeFileChunk(bytes: Uint8Array): string {
  let text = ''; for (let offset = 0; offset < bytes.length; offset += 16384) text += String.fromCharCode(...bytes.subarray(offset, offset + 16384));
  return btoa(text);
}
export function decodeFileChunk(data: string): Uint8Array { return Uint8Array.from(atob(data), char => char.charCodeAt(0)); }

/** Streams a picker file or editor contents. The caller supplies bounded slices, never a whole-file base64 request. */
export async function uploadFileChunks(api: FileTransferApi, input: FileTransferInput<'beginFileWrite'>, read: (offset: number, length: number) => Promise<Uint8Array>, options: { signal?: AbortSignal; progress?: (offset: number) => void } = {}): Promise<Result<FileWriteResult>> {
  if (options.signal?.aborted) return failure('ACCESS_DENIED', 'File transfer cancelled.');
  const begin = await api.beginWrite(input); if (!begin.ok) return begin;
  const transferId = begin.value.transferId;
  try {
    let offset = 0;
    while (offset < input.size) {
      if (options.signal?.aborted) return failure('ACCESS_DENIED', 'File transfer cancelled.');
      const bytes = await read(offset, Math.min(FILE_CHUNK_BYTES, input.size - offset));
      if (bytes.length !== Math.min(FILE_CHUNK_BYTES, input.size - offset)) return failure('VALIDATION', 'Local file slice does not match the declared file size.');
      const saved = await api.writeChunk({ transferId, offset, data: encodeFileChunk(bytes) }); if (!saved.ok) return saved;
      if (saved.value.offset !== offset + bytes.length) return failure('INTERNAL', 'Host returned an invalid file offset.');
      offset = saved.value.offset; options.progress?.(offset);
    }
    if (options.signal?.aborted) return failure('ACCESS_DENIED', 'File transfer cancelled.');
    return await api.finishWrite({ transferId });
  } catch { return failure('STORAGE_FAILED', 'File upload failed. No uncertain operation was retried.', true); }
  finally { await api.cancelTransfer({ transferId }).catch(() => {}); }
}
/** Streams a host file into a platform download sink. Callers may write each chunk straight to disk. */
export async function downloadFileChunks(api: FileTransferApi, location: FileLocation, write: (bytes: Uint8Array, offset: number) => Promise<void>, options: { signal?: AbortSignal; progress?: (offset: number) => void } = {}): Promise<Result<{ path: string; size: number; revision: string }>> {
  if (options.signal?.aborted) return failure('ACCESS_DENIED', 'File transfer cancelled.');
  const begin = await api.beginRead(location); if (!begin.ok) return begin;
  const { transferId, size } = begin.value;
  try {
    let offset = 0;
    do {
      if (options.signal?.aborted) return failure('ACCESS_DENIED', 'File transfer cancelled.');
      const chunk = await api.readChunk({ transferId, offset }); if (!chunk.ok) return chunk;
      const bytes = decodeFileChunk(chunk.value.data);
      if (bytes.length > FILE_CHUNK_BYTES || offset + bytes.length !== chunk.value.offset || chunk.value.offset > size || chunk.value.eof !== (chunk.value.offset === size) || (!bytes.length && !chunk.value.eof)) return failure('INTERNAL', 'Invalid host file chunk.');
      if (bytes.length) await write(bytes, offset); offset = chunk.value.offset; options.progress?.(offset);
      if (chunk.value.eof) break;
    } while (offset < size);
    return success({ path: begin.value.path, size, revision: begin.value.revision });
  } catch { return failure('STORAGE_FAILED', 'File download failed.', true); }
  finally { await api.cancelTransfer({ transferId }).catch(() => {}); }
}
export function createFileApi(wire: FileWire): FileApi & FileTransferApi {
  async function read(location: FileLocation, assetPath?: string): Promise<Result<FileRead | FileAsset>> {
    if (!fileLocationSchema.safeParse(location).success) return failure('VALIDATION', 'Invalid file location.');
    const begin = await wire.beginRead({ ...location, ...(assetPath === undefined ? {} : { assetPath }) }); if (!begin.ok) return begin;
    const { transferId, size, kind, path, mimeType, revision } = begin.value;
    try {
      const limit = assetPath !== undefined || kind === 'image' ? FILE_IMAGE_BYTES : FILE_TEXT_BYTES;
      if (kind === 'binary' && assetPath === undefined) return success(fileReadSchema.parse({ kind, path, mimeType: 'application/octet-stream', size, revision }));
      if (size > limit) return failure('UNSUPPORTED', kind === 'text' && assetPath === undefined ? 'Text preview is limited to 8 MiB. Use Download for larger files.' : 'Image and asset preview is limited to 64 MiB. Use Download for larger files.');
      const bytes = new Uint8Array(size); let offset = 0;
      do {
        const chunk = await wire.readChunk({ transferId, offset }); if (!chunk.ok) return chunk;
        const part = decodeFileChunk(chunk.value.data);
        if (part.length > FILE_CHUNK_BYTES || part.length + offset !== chunk.value.offset || chunk.value.offset > size || chunk.value.eof !== (chunk.value.offset === size) || (!part.length && !chunk.value.eof)) return failure('INTERNAL', 'Invalid preview chunk.');
        bytes.set(part, offset); offset = chunk.value.offset; if (chunk.value.eof) break;
      } while (offset < size);
      if (assetPath !== undefined) return success(fileAssetSchema.parse({ mimeType, size, dataUrl: `data:${mimeType};base64,${encodeFileChunk(bytes)}` }));
      if (kind === 'text') return success(fileReadSchema.parse({ kind, path, size, mimeType, revision, text: decodeFileUtf8(bytes) }));
      return success(fileReadSchema.parse({ kind, path, size, mimeType, revision, dataUrl: `data:${mimeType};base64,${encodeFileChunk(bytes)}` }));
    } catch { return failure('INTERNAL', 'Invalid preview data received.'); }
    finally { await wire.cancelTransfer({ transferId }).catch(() => {}); }
  }
  const api: FileApi & FileTransferApi = {
    ...wire,
    read: async input => { const result = await read(input); if (!result.ok) return result; const parsed = fileReadSchema.safeParse(result.value); return parsed.success ? success(parsed.data) : failure('INTERNAL', 'Invalid file preview.'); },
    readAsset: async input => { const checked = fileAssetRequestSchema.safeParse(input); if (!checked.success) return failure('VALIDATION', 'Invalid preview asset location.'); const { assetPath, ...location } = checked.data, result = await read(location, assetPath); if (!result.ok) return result; const parsed = fileAssetSchema.safeParse(result.value); return parsed.success ? success(parsed.data) : failure('INTERNAL', 'Invalid preview asset.'); },
    write: async input => {
      if (typeof input?.text !== 'string' || !isWellFormedFileText(input.text)) return failure('VALIDATION', 'Text contains an unpaired UTF-16 surrogate and cannot be saved.');
      if (!fileEditorWriteSchema.safeParse(input).success) return failure('VALIDATION', 'Invalid text save request. Text editing supports UTF-8 up to 8 MiB.');
      const bytes = new TextEncoder().encode(input.text); if (bytes.length > FILE_TEXT_BYTES || input.text.includes('\0')) return failure('VALIDATION', 'Text exceeds the 8 MiB editor limit or contains NUL bytes.');
      const { text: _text, ...location } = input;
      return uploadFileChunks(wire, { ...location, size: bytes.length, mode: 'replace', content: 'text' }, async (offset, length) => bytes.subarray(offset, offset + length));
    },
    writeImage: async input => {
      if (!fileEditorImageWriteSchema.safeParse(input).success) return failure('VALIDATION', 'Invalid image save request. PNG/JPEG/WebP editing supports up to 64 MiB.');
      const match = /^data:(image\/(?:png|jpeg|webp));base64,/.exec(input.dataUrl); if (!match) return failure('VALIDATION', 'Only PNG, JPEG and WebP image saves are supported.');
      const encoded = input.dataUrl.slice(match[0].length), size = encoded.length / 4 * 3 - (encoded.endsWith('==') ? 2 : encoded.endsWith('=') ? 1 : 0);
      if (!Number.isSafeInteger(size) || size <= 0 || size > FILE_IMAGE_BYTES) return failure('VALIDATION', 'Edited image exceeds the 64 MiB editor limit.');
      const type = match[1]; if (type !== 'image/png' && type !== 'image/jpeg' && type !== 'image/webp') return failure('VALIDATION', 'Invalid image type.');
      const { dataUrl: _dataUrl, ...location } = input;
      return uploadFileChunks(wire, { ...location, size, mode: 'replace', content: 'image', mimeType: type }, async (offset, length) => decodeFileChunk(encoded.slice(offset / 3 * 4, Math.ceil((offset + length) / 3) * 4)));
    },
  };
  return api;
}
