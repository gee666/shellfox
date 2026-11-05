import { failure, success } from '../../src/shared/contracts';
import type { FileRead } from '../../src/shared/file-contracts';
import type { FileTransferApi } from '../../src/shared/file-transfer-contracts';
import { FILE_CHUNK_BYTES } from '../../src/shared/file-transfer-schemas';
import { decodeFileChunk, encodeFileChunk } from '../../src/shared/file-api';

export const nativeDownloadSize = FILE_CHUNK_BYTES * 2 + 19;
export const nativeLargeSize = 64 * 1024 * 1024 + 17;
const revision = 'a'.repeat(64);
export const transferFixtures: FileRead[] = [
  { kind: 'binary', path: '/fixture/shellfox-transfer-download.bin', size: nativeDownloadSize, revision, mimeType: 'application/octet-stream' },
  { kind: 'binary', path: '/fixture/shellfox-transfer-empty.bin', size: 0, revision, mimeType: 'application/octet-stream' },
  { kind: 'binary', path: '/fixture/shellfox-transfer-large.bin', size: nativeLargeSize, revision, mimeType: 'application/octet-stream' },
];
const delay = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms));

/** Test-owned streaming endpoint. Native pickers/Downloads IO are real; remote bytes are deterministic. */
export function fixtureFileTransfers(files: Map<string, FileRead>, report: (message: string) => void): FileTransferApi {
  let nextId = 0;
  const reads = new Map<string, { size: number; path: string; data?: Uint8Array }>();
  const writes = new Map<string, { size: number; path: string; offset: number; chunks: number }>();
  const id = () => `30000000-0000-4000-8000-${String(++nextId).padStart(12, '0')}`;
  return {
    beginRead: async input => {
      const file = files.get(input.path);
      if (!file) return failure('NOT_FOUND', 'Fixture download missing.');
      const data = file.kind === 'text' ? new TextEncoder().encode(file.text)
        : file.kind === 'image' ? decodeFileChunk(file.dataUrl.slice(file.dataUrl.indexOf(',') + 1)) : undefined;
      const size = data?.length ?? file.size;
      const transferId = id(); reads.set(transferId, { size, path: file.path, data });
      return success({ transferId, path: file.path, size, revision: file.revision, mimeType: file.mimeType, kind: file.kind, chunkBytes: FILE_CHUNK_BYTES });
    },
    readChunk: async input => {
      const read = reads.get(input.transferId);
      if (!read || input.offset > read.size) return failure('NOT_FOUND', 'Fixture transfer expired.');
      if (read.path.endsWith('download.bin')) await delay(1200);
      const length = Math.min(FILE_CHUNK_BYTES, read.size - input.offset);
      const bytes = read.data?.subarray(input.offset, input.offset + length) ?? Uint8Array.from({ length }, (_, index) => (input.offset + index) % 251);
      return success({ offset: input.offset + length, data: encodeFileChunk(bytes), eof: input.offset + length === read.size });
    },
    beginWrite: async input => {
      if (input.mode !== 'create' || !input.path.startsWith('/fixture/') || input.path.slice('/fixture/'.length).includes('/')) return failure('ACCESS_DENIED', 'Wrong fixture upload folder.');
      if (files.has(input.path)) return failure('VALIDATION', 'The selected remote file already exists.');
      const transferId = id(); writes.set(transferId, { path: input.path, size: input.size, offset: 0, chunks: 0 });
      return success({ transferId, chunkBytes: FILE_CHUNK_BYTES });
    },
    writeChunk: async input => {
      await delay(1200);
      const write = writes.get(input.transferId);
      if (!write || input.offset !== write.offset) return failure('NOT_FOUND', 'Fixture write expired.');
      const bytes = atob(input.data);
      for (let index = 0; index < bytes.length; index++) if (bytes.charCodeAt(index) !== (input.offset + index) % 251) return failure('VALIDATION', 'Native picker bytes changed.');
      write.offset += bytes.length; write.chunks++;
      return success({ offset: write.offset });
    },
    finishWrite: async input => {
      const write = writes.get(input.transferId);
      if (!write || write.offset !== write.size) return failure('VALIDATION', 'Incomplete fixture upload.');
      writes.delete(input.transferId);
      files.set(write.path, { kind: 'binary', path: write.path, size: write.size, revision, mimeType: 'application/octet-stream' });
      report(`Uploaded fixture; bytes ${write.size}; chunks ${write.chunks}; exact`);
      return success({ path: write.path, size: write.size, revision });
    },
    cancelTransfer: async input => { reads.delete(input.transferId); writes.delete(input.transferId); return success({ cancelled: true }); },
  };
}
