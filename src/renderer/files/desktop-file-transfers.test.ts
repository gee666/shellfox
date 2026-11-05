import { describe, expect, it } from 'vitest';
import { failure, success } from '../../shared/contracts';
import { FILE_CHUNK_BYTES } from '../../shared/file-transfer-schemas';
import { downloadDesktopFile, uploadDesktopFile } from './desktop-file-transfers';
import { hostId, localId, transferFixture } from './file-transfer-test-fixtures';

const location = { sessionId: '00000000-0000-4000-8000-000000000003', path: '/folder' };
const progress = () => {};
describe('desktop file transfer pipes', () => {
  it('streams create-only upload chunks to the snapshotted folder without whole-file requests', async () => {
    const f = transferFixture(Buffer.alloc(FILE_CHUNK_BYTES * 2 + 7, 53));
    expect(await uploadDesktopFile(f.remote, f.local, location, new AbortController().signal, progress)).toBe('upload.bin');
    expect(f.remote.beginWrite).toHaveBeenCalledWith({ ...location, path: '/folder/upload.bin', size: f.bytes.length, mode: 'create', content: 'file' });
    expect(f.remote.writeChunk.mock.calls.map(([input]) => input.offset)).toEqual([0, FILE_CHUNK_BYTES, FILE_CHUNK_BYTES * 2]);
    expect(f.remote.finishWrite).toHaveBeenCalledOnce(); expect(f.local.releaseUpload).toHaveBeenCalledWith({ handle: localId });
  });
  it('streams downloads with next-offset validation and releases the first lease before a native chooser', async () => {
    const f = transferFixture(Buffer.alloc(FILE_CHUNK_BYTES + 5, 84));
    f.local.beginDownload.mockImplementationOnce(async () => { expect(f.remote.cancelTransfer).toHaveBeenCalledWith({ transferId: hostId }); return success({ handle: localId }); });
    expect(await downloadDesktopFile(f.remote, f.local, { ...location, path: '/folder/source.bin' }, new AbortController().signal, progress)).toBe('source.bin');
    expect(f.remote.beginRead).toHaveBeenCalledTimes(2);
    expect(f.local.writeDownload.mock.calls.map(([input]) => input.offset)).toEqual([0, FILE_CHUNK_BYTES]);
    expect(f.local.finishDownload).toHaveBeenCalledOnce(); expect(f.remote.cancelTransfer).toHaveBeenCalled();
  });
  it('uploads and downloads zero-byte files without any writeChunk calls', async () => {
    const f = transferFixture(Buffer.alloc(0)), signal = new AbortController().signal;
    expect(await uploadDesktopFile(f.remote, f.local, location, signal, progress)).toBe('upload.bin');
    expect(f.remote.beginWrite).toHaveBeenCalledOnce(); expect(f.remote.writeChunk).not.toHaveBeenCalled(); expect(f.remote.finishWrite).toHaveBeenCalledOnce();
    expect(await downloadDesktopFile(f.remote, f.local, { ...location, path: '/empty' }, signal, progress)).toBe('empty');
    expect(f.remote.readChunk).toHaveBeenCalledWith({ transferId: hostId, offset: 0 });
    expect(f.local.writeDownload).not.toHaveBeenCalled(); expect(f.local.finishDownload).toHaveBeenCalledOnce();
  });
  it('does not create remote transfers after an already cancelled action or picker cancellation', async () => {
    const f = transferFixture(), controller = new AbortController(); controller.abort();
    await expect(uploadDesktopFile(f.remote, f.local, location, controller.signal, progress)).rejects.toThrow('cancelled');
    expect(f.local.pickUpload).not.toHaveBeenCalled();
    f.local.pickUpload.mockResolvedValueOnce(success(null));
    expect(await uploadDesktopFile(f.remote, f.local, location, new AbortController().signal, progress)).toBeNull();
    expect(f.remote.beginWrite).not.toHaveBeenCalled();
  });
  it('releases a local source if cancellation occurs while the picker is open', async () => {
    const f = transferFixture(), controller = new AbortController();
    f.local.pickUpload.mockImplementationOnce(async () => { controller.abort(); return success({ handle: localId, name: 'upload.bin', size: 0, chunkBytes: FILE_CHUNK_BYTES }); });
    await expect(uploadDesktopFile(f.remote, f.local, location, controller.signal, progress)).rejects.toThrow('cancelled');
    expect(f.local.releaseUpload).toHaveBeenCalledWith({ handle: localId }); expect(f.remote.beginWrite).not.toHaveBeenCalled();
  });
  it('surfaces no-clobber conflicts and never retries uncertain writes', async () => {
    const f = transferFixture(); f.remote.beginWrite.mockResolvedValueOnce(failure('FILE_CONFLICT', 'A file already exists at this destination.'));
    await expect(uploadDesktopFile(f.remote, f.local, location, new AbortController().signal, progress)).rejects.toThrow('already exists');
    expect(f.remote.beginWrite).toHaveBeenCalledOnce(); expect(f.remote.finishWrite).not.toHaveBeenCalled(); expect(f.local.releaseUpload).toHaveBeenCalledOnce();
  });
  it('cleans both sides when approval is revoked after the native save dialog', async () => {
    const f = transferFixture();
    f.local.beginDownload.mockImplementationOnce(async () => { f.remote.beginRead.mockResolvedValueOnce(failure('AUTH_FAILED', 'Host approval was revoked.')); return success({ handle: localId }); });
    await expect(downloadDesktopFile(f.remote, f.local, { ...location, path: '/source' }, new AbortController().signal, progress)).rejects.toThrow('revoked');
    expect(f.local.cancelDownload).toHaveBeenCalledWith({ handle: localId }); expect(f.local.finishDownload).not.toHaveBeenCalled();
  });
  it('rejects files changed while the save dialog was open instead of writing a mismatched snapshot', async () => {
    const f = transferFixture();
    f.local.beginDownload.mockImplementationOnce(async () => {
      f.remote.beginRead.mockImplementationOnce(async input => success({ transferId: hostId, path: input.path, size: f.bytes.length + 1, revision: 'c'.repeat(64), kind: 'binary', mimeType: 'application/octet-stream', chunkBytes: FILE_CHUNK_BYTES }));
      return success({ handle: localId });
    });
    await expect(downloadDesktopFile(f.remote, f.local, { ...location, path: '/source' }, new AbortController().signal, progress)).rejects.toThrow('changed');
    expect(f.remote.readChunk).not.toHaveBeenCalled(); expect(f.local.cancelDownload).toHaveBeenCalledWith({ handle: localId });
  });
  it('cancels both transfer owners immediately during an in-flight chunk without retrying it', async () => {
    const f = transferFixture(), controller = new AbortController();
    f.remote.writeChunk.mockImplementationOnce(async () => { controller.abort(); return failure('ACCESS_DENIED', 'Transfer cancelled.'); });
    await expect(uploadDesktopFile(f.remote, f.local, location, controller.signal, progress)).rejects.toThrow('cancelled');
    expect(f.remote.writeChunk).toHaveBeenCalledOnce(); expect(f.remote.cancelTransfer).toHaveBeenCalledWith({ transferId: hostId }); expect(f.local.releaseUpload).toHaveBeenCalledWith({ handle: localId });
    expect(f.remote.finishWrite).not.toHaveBeenCalled();
  });
  it('rejects invalid next offsets rather than repeating or committing chunks', async () => {
    const f = transferFixture(); f.remote.readChunk.mockResolvedValueOnce(success({ offset: 0, data: 'YQ==', eof: false }));
    await expect(downloadDesktopFile(f.remote, f.local, { ...location, path: '/source' }, new AbortController().signal, progress)).rejects.toThrow('Invalid host');
    expect(f.local.finishDownload).not.toHaveBeenCalled(); expect(f.remote.readChunk).toHaveBeenCalledOnce();
  });
});
