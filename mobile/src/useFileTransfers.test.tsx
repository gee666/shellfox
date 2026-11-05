// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { failure, success } from '../../src/shared/contracts';
import type { FileApi } from '../../src/shared/file-contracts';
import type { FileTransferApi } from '../../src/shared/file-transfer-contracts';
import { FILE_CHUNK_BYTES } from '../../src/shared/file-transfer-schemas';
import { deferred } from './client/test-fixtures';
import { useFileTransfers } from './useFileTransfers';

const native = vi.hoisted(() => ({ pick: vi.fn(), read: vi.fn(), release: vi.fn(), cancelPick: vi.fn(), begin: vi.fn(), write: vi.fn(), finish: vi.fn(), cancel: vi.fn() }));
vi.mock('./files', () => ({ pickedFiles: { pick: native.pick, read: native.read, release: native.release, cancelPick: native.cancelPick } }));
vi.mock('./downloads', () => ({ downloads: { begin: native.begin, write: native.write, finish: native.finish, cancel: native.cancel } }));
const id = '10000000-0000-4000-8000-000000000001';
function api(): FileApi & FileTransferApi {
  return {
    list: async () => success({ path: '/host', parentPath: '/', entries: [], truncated: false }),
    read: async () => failure('UNSUPPORTED', 'No preview'), write: async () => failure('UNSUPPORTED', 'No editor'),
    writeImage: async () => failure('UNSUPPORTED', 'No image editor'), readAsset: async () => failure('NOT_FOUND', 'No asset'),
    beginRead: async () => success({ transferId: id, path: '/host/empty.bin', size: 0, revision: 'a'.repeat(64), kind: 'binary', mimeType: 'application/octet-stream', chunkBytes: FILE_CHUNK_BYTES }),
    readChunk: async () => success({ offset: 0, data: '', eof: true }),
    beginWrite: async () => success({ transferId: id, chunkBytes: FILE_CHUNK_BYTES }), writeChunk: async () => success({ offset: 0 }),
    finishWrite: async () => success({ path: '/host/empty.bin', size: 0, revision: 'a'.repeat(64) }),
    cancelTransfer: async () => success({ cancelled: true }),
  };
}
beforeEach(() => {
  vi.resetAllMocks(); native.pick.mockResolvedValue({ handle: id, fileName: 'empty.bin', size: 0 });
  native.begin.mockResolvedValue({ handle: id }); native.finish.mockResolvedValue({ fileName: 'empty.bin', location: 'Downloads' });
  native.release.mockResolvedValue(undefined); native.cancelPick.mockResolvedValue(undefined); native.cancel.mockResolvedValue(undefined);
});
afterEach(cleanup);
it.each(['upload', 'download'] as const)('never reports old %s success after the host changes during finally cleanup', async kind => {
  const original = api(), replacement = api(), cancelled = deferred<Awaited<ReturnType<FileTransferApi['cancelTransfer']>>>();
  const cancel = vi.spyOn(original, 'cancelTransfer').mockReturnValue(cancelled.promise);
  const uploaded = vi.fn();
  const { result, rerender } = renderHook(({ currentApi, epoch }) => useFileTransfers({ api: currentApi, context: {}, available: true, connectionEpoch: epoch, uploaded }),
    { initialProps: { currentApi: original, epoch: 'original' } });
  await act(async () => { result.current[kind]('/host/empty.bin'); });
  await waitFor(() => expect(cancel).toHaveBeenCalled());
  rerender({ currentApi: replacement, epoch: 'replacement' });
  await act(async () => { cancelled.resolve(success({ cancelled: true })); });
  expect(result.current.message).toBeNull(); expect(result.current.progress).toBeNull(); expect(uploaded).not.toHaveBeenCalled();
});
it('retires an in-flight picker when approval is revoked', async () => {
  const picked = deferred<unknown>(); native.pick.mockReturnValue(picked.promise);
  const currentApi = api(), begin = vi.spyOn(currentApi, 'beginWrite');
  const { result, rerender } = renderHook(({ available }) => useFileTransfers({ api: currentApi, context: {}, available, uploaded: vi.fn() }), { initialProps: { available: true } });
  await act(async () => { result.current.upload('/host'); });
  rerender({ available: false });
  await act(async () => { picked.resolve({ handle: id, fileName: 'empty.bin', size: 0 }); });
  expect(begin).not.toHaveBeenCalled(); expect(native.release).toHaveBeenCalledWith(id);
  expect(result.current.message).toBe('Transfer cancelled.');
});
