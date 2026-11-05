import { vi } from 'vitest';
import { success } from '../../shared/contracts';
import type { FileTransferApi } from '../../shared/file-transfer-contracts';
import type { DesktopFilesApi } from '../../shared/desktop-file-contracts';
import { FILE_CHUNK_BYTES } from '../../shared/file-transfer-schemas';

export const hostId = '00000000-0000-4000-8000-000000000001';
export const localId = '00000000-0000-4000-8000-000000000002';
export function transferFixture(bytes = Buffer.from('file contents')) {
  const remote = {
    beginRead: vi.fn<FileTransferApi['beginRead']>(async input => success({ transferId: hostId, path: input.path, size: bytes.length, revision: 'a'.repeat(64), kind: 'binary', mimeType: 'application/octet-stream', chunkBytes: FILE_CHUNK_BYTES })),
    readChunk: vi.fn<FileTransferApi['readChunk']>(async input => {
      const part = bytes.subarray(input.offset, input.offset + FILE_CHUNK_BYTES), offset = input.offset + part.length;
      return success({ offset, data: part.toString('base64'), eof: offset === bytes.length });
    }),
    beginWrite: vi.fn<FileTransferApi['beginWrite']>(async () => success({ transferId: hostId, chunkBytes: FILE_CHUNK_BYTES })),
    writeChunk: vi.fn<FileTransferApi['writeChunk']>(async input => success({ offset: input.offset + Buffer.from(input.data, 'base64').length })),
    finishWrite: vi.fn<FileTransferApi['finishWrite']>(async () => success({ path: '/folder/upload.bin', size: bytes.length, revision: 'b'.repeat(64) })),
    cancelTransfer: vi.fn<FileTransferApi['cancelTransfer']>(async () => success({ cancelled: true })),
  };
  const local = {
    pickUpload: vi.fn<DesktopFilesApi['pickUpload']>(async () => success({ handle: localId, name: 'upload.bin', size: bytes.length, chunkBytes: FILE_CHUNK_BYTES })),
    readUpload: vi.fn<DesktopFilesApi['readUpload']>(async input => {
      const part = bytes.subarray(input.offset, input.offset + FILE_CHUNK_BYTES), offset = input.offset + part.length;
      return success({ offset, data: part.toString('base64'), eof: offset === bytes.length });
    }),
    releaseUpload: vi.fn<DesktopFilesApi['releaseUpload']>(async () => success({ released: true })),
    beginDownload: vi.fn<DesktopFilesApi['beginDownload']>(async () => success({ handle: localId })),
    writeDownload: vi.fn<DesktopFilesApi['writeDownload']>(async input => success({ offset: input.offset + Buffer.from(input.data, 'base64').length })),
    finishDownload: vi.fn<DesktopFilesApi['finishDownload']>(async () => success({ saved: true })),
    cancelDownload: vi.fn<DesktopFilesApi['cancelDownload']>(async () => success({ cancelled: true })),
  };
  return { remote, local, bytes };
}
