import { beforeEach, expect, it, vi } from 'vitest';
import { FILE_CHUNK_BYTES } from '../shared/file-transfer-schemas';
import { DESKTOP_FILES_CHANNEL } from '../shared/desktop-file-schemas';
const state = vi.hoisted(() => ({ invoke: vi.fn<(channel: string, request: unknown) => Promise<unknown>>() }));
vi.mock('electron', () => ({ ipcRenderer: { invoke: state.invoke } }));
import { desktopFilesApi } from './desktop-files';
const handle = '00000000-0000-4000-8000-000000000001';
beforeEach(() => state.invoke.mockReset());
it('uses only the local narrow channel and validates path-free picker responses', async () => {
  state.invoke.mockResolvedValueOnce({ ok: true, value: { handle, name: 'file.bin', size: 1, chunkBytes: FILE_CHUNK_BYTES } });
  expect(await desktopFilesApi.pickUpload()).toMatchObject({ ok: true, value: { handle } });
  expect(state.invoke).toHaveBeenCalledExactlyOnceWith(DESKTOP_FILES_CHANNEL, { version: 1, method: 'pickUpload', payload: {} });
  state.invoke.mockResolvedValueOnce({ ok: true, value: { handle, name: 'file.bin', size: 1, chunkBytes: FILE_CHUNK_BYTES, path: '/private/file.bin' } });
  expect(await desktopFilesApi.pickUpload()).toMatchObject({ ok: false, error: { code: 'INTERNAL' } });
});
it('rejects arbitrary renderer paths, malformed handles and oversized chunks before IPC', async () => {
  expect(await desktopFilesApi.beginDownload({ name: '../escape', size: 1 })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await desktopFilesApi.readUpload({ handle: '/etc/passwd', offset: 0 })).toMatchObject({ ok: false });
  expect(await desktopFilesApi.writeDownload({ handle, offset: 0, data: Buffer.alloc(FILE_CHUNK_BYTES + 1).toString('base64') })).toMatchObject({ ok: false });
  expect(state.invoke).not.toHaveBeenCalled();
});
it('accepts zero-byte end-of-file and reports bridge failures without retry', async () => {
  state.invoke.mockResolvedValueOnce({ ok: true, value: { offset: 0, data: '', eof: true } });
  expect(await desktopFilesApi.readUpload({ handle, offset: 0 })).toEqual({ ok: true, value: { offset: 0, data: '', eof: true } });
  state.invoke.mockRejectedValueOnce(new Error('Window destroyed'));
  expect(await desktopFilesApi.finishDownload({ handle })).toMatchObject({ ok: false, error: { code: 'STORAGE_FAILED' } });
  expect(state.invoke).toHaveBeenCalledTimes(2);
});
