import { expect, it, vi } from 'vitest';
import { failure, success } from '../../src/shared/contracts';
import { FILE_CHUNK_BYTES } from '../../src/shared/file-transfer-schemas';
import type { FileTransferApi } from '../../src/shared/file-transfer-contracts';
import { FileTransferCancelled, MobileFileTransfer } from './file-transfer';
import type { PickedFiles } from './files';
import type { Downloads } from './downloads';
import { deferred } from './client/test-fixtures';

const id = '10000000-0000-4000-8000-000000000001', token = '20000000-0000-4000-8000-000000000001';
function setup(bytes = Buffer.from('native bytes')) {
  const api = {
    beginRead: vi.fn<FileTransferApi['beginRead']>(async () => success({ transferId: id, path: '/host/file.bin', size: bytes.length, revision: 'a'.repeat(64), mimeType: 'application/octet-stream', kind: 'binary', chunkBytes: FILE_CHUNK_BYTES })),
    readChunk: vi.fn<FileTransferApi['readChunk']>(async ({ offset }) => { const part = bytes.subarray(offset, offset + FILE_CHUNK_BYTES); return success({ data: part.toString('base64'), offset: offset + part.length, eof: offset + part.length === bytes.length }); }),
    beginWrite: vi.fn<FileTransferApi['beginWrite']>(async () => success({ transferId: id, chunkBytes: FILE_CHUNK_BYTES })),
    writeChunk: vi.fn<FileTransferApi['writeChunk']>(async input => success({ offset: input.offset + Buffer.from(input.data, 'base64').length })),
    finishWrite: vi.fn<FileTransferApi['finishWrite']>(async () => success({ path: '/host/file.bin', size: bytes.length, revision: 'b'.repeat(64) })),
    cancelTransfer: vi.fn<FileTransferApi['cancelTransfer']>(async () => success({ cancelled: true })),
  };
  const files = {
    pick: vi.fn<PickedFiles['pick']>(async () => ({ handle: id, fileName: 'file.bin', size: bytes.length })),
    read: vi.fn<PickedFiles['read']>(async (_handle, offset, length) => ({ data: bytes.subarray(offset, offset + length).toString('base64'), bytes: length })),
    release: vi.fn<PickedFiles['release']>(async () => {}), cancelPick: vi.fn<PickedFiles['cancelPick']>(async () => {}),
  };
  const downloads = {
    begin: vi.fn<Downloads['begin']>(async () => ({ handle: token })),
    write: vi.fn<Downloads['write']>(async (_handle, offset, data) => ({ offset: offset + Buffer.from(data, 'base64').length })),
    finish: vi.fn<Downloads['finish']>(async () => ({ fileName: 'file.bin', location: 'Downloads' })),
    cancel: vi.fn<Downloads['cancel']>(async () => {}),
  };
  const current = vi.fn(() => true);
  const transfer = new MobileFileTransfer({ api, files, downloads, current, token, context: { sessionId: id, tabId: token }, progress: vi.fn() });
  return { transfer, api, files, downloads, current };
}
it('uploads bounded chunks to the captured folder without overwriting existing files', async () => {
  const bytes = Buffer.alloc(FILE_CHUNK_BYTES + 17, 127), f = setup(bytes);
  expect(await f.transfer.upload('C:\\host\\folder')).toBe('/host/file.bin');
  expect(f.api.beginWrite).toHaveBeenCalledWith({ sessionId: id, tabId: token, path: 'C:\\host\\folder/file.bin', size: bytes.length, mode: 'create', content: 'file' });
  expect(f.api.writeChunk).toHaveBeenCalledTimes(2);
  expect(f.files.release).toHaveBeenCalledExactlyOnceWith(id);
  expect(f.api.cancelTransfer).toHaveBeenCalledWith({ transferId: id });
});
it('downloads each binary chunk directly to the native sink without assembling the file in JS', async () => {
  const bytes = Buffer.alloc(FILE_CHUNK_BYTES + 1, 231), f = setup(bytes);
  await expect(f.transfer.download('/selected/large.bin')).resolves.toEqual({ fileName: 'file.bin', location: 'Downloads' });
  expect(f.api.beginRead).toHaveBeenCalledWith({ sessionId: id, tabId: token, path: '/selected/large.bin' });
  expect(f.downloads.write.mock.calls.map(call => Buffer.from(call[2], 'base64').length)).toEqual([FILE_CHUNK_BYTES, 1]);
  expect(f.downloads.cancel).not.toHaveBeenCalled();
});
it('uploads and commits an empty file without reading or writing a chunk', async () => {
  const f = setup(Buffer.alloc(0));
  expect(await f.transfer.upload('/host')).toBe('/host/file.bin');
  expect(f.files.read).not.toHaveBeenCalled(); expect(f.api.writeChunk).not.toHaveBeenCalled();
  expect(f.api.finishWrite).toHaveBeenCalledWith({ transferId: id });
});
it('downloads and commits an empty native file with eof at offset zero and no native write', async () => {
  const f = setup(Buffer.alloc(0));
  await f.transfer.download('/host/empty');
  expect(f.api.readChunk).toHaveBeenCalledExactlyOnceWith({ transferId: id, offset: 0 });
  expect(f.downloads.write).not.toHaveBeenCalled(); expect(f.downloads.finish).toHaveBeenCalledWith(token);
});
it('cancels before begin and after picker completion, releasing any late selection', async () => {
  const f = setup(); f.transfer.cancel();
  await expect(f.transfer.upload('/host')).rejects.toBeInstanceOf(FileTransferCancelled);
  expect(f.files.pick).not.toHaveBeenCalled(); expect(f.api.beginWrite).not.toHaveBeenCalled();
  const second = setup(), chosen = deferred<Awaited<ReturnType<PickedFiles['pick']>>>();
  second.files.pick.mockReturnValue(chosen.promise);
  const pending = second.transfer.upload('/host'); second.transfer.cancel();
  chosen.resolve({ handle: id, fileName: 'file.bin', size: 5 });
  await expect(pending).rejects.toBeInstanceOf(FileTransferCancelled);
  expect(second.files.release).toHaveBeenCalledWith(id); expect(second.api.beginWrite).not.toHaveBeenCalled();
});
it('cleans up a late begin response after scope changes before native creation', async () => {
  const f = setup(), begin = deferred<Awaited<ReturnType<FileTransferApi['beginRead']>>>();
  f.api.beginRead.mockReturnValue(begin.promise);
  const downloading = f.transfer.download('/host/file'); f.current.mockReturnValue(false);
  begin.resolve(success({ transferId: id, path: '/host/file', size: 5, revision: 'a'.repeat(64), kind: 'binary', mimeType: 'application/octet-stream', chunkBytes: FILE_CHUNK_BYTES }));
  await expect(downloading).rejects.toBeInstanceOf(FileTransferCancelled);
  expect(f.downloads.begin).not.toHaveBeenCalled(); expect(f.api.cancelTransfer).toHaveBeenCalledWith({ transferId: id });
});
it.each(['ACCESS_DENIED', 'FILE_CONFLICT'] as const)('does not retry %s or publish a partial download', async code => {
  const f = setup(); f.api.readChunk.mockResolvedValue(failure(code, 'Host access changed.'));
  await expect(f.transfer.download('/host/file')).rejects.toThrow('Host access changed.');
  expect(f.api.readChunk).toHaveBeenCalledOnce(); expect(f.downloads.finish).not.toHaveBeenCalled();
  expect(f.downloads.cancel).toHaveBeenCalledWith(token); expect(f.api.cancelTransfer).toHaveBeenCalledWith({ transferId: id });
});
it('honors a positive upload commit ACK when cancellation races finish', async () => {
  const f = setup(Buffer.alloc(0)), committed = deferred<Awaited<ReturnType<FileTransferApi['finishWrite']>>>();
  f.api.finishWrite.mockReturnValue(committed.promise);
  const uploading = f.transfer.upload('/host');
  await vi.waitFor(() => expect(f.api.finishWrite).toHaveBeenCalled());
  f.transfer.cancel(); committed.resolve(success({ path: '/host/file.bin', size: 0, revision: 'b'.repeat(64) }));
  await expect(uploading).resolves.toBe('/host/file.bin');
});
it('honors a positive native download commit ACK and never cleans up the published file afterward', async () => {
  const f = setup(Buffer.alloc(0)), committed = deferred<Awaited<ReturnType<Downloads['finish']>>>();
  f.downloads.finish.mockReturnValue(committed.promise);
  const downloading = f.transfer.download('/host/file.bin');
  await vi.waitFor(() => expect(f.downloads.finish).toHaveBeenCalled());
  f.transfer.cancel(); committed.resolve({ fileName: 'file.bin', location: 'Downloads' });
  await expect(downloading).resolves.toEqual({ fileName: 'file.bin', location: 'Downloads' });
  expect(f.downloads.cancel).toHaveBeenCalledTimes(1);
});
it('rejects unsafe picker basenames and never starts a remote write', async () => {
  const f = setup(); f.files.pick.mockResolvedValue({ handle: id, fileName: '../other', size: 1 });
  await expect(f.transfer.upload('/host')).rejects.toThrow('invalid name');
  expect(f.api.beginWrite).not.toHaveBeenCalled(); expect(f.files.release).toHaveBeenCalledWith(id);
});
