import { beforeEach, expect, it, vi } from 'vitest';
import { failure, success } from '../../../src/shared/contracts';
import type { PickedFile, PickedFiles } from '../files';
import { TerminalUpload, UploadCancelled, type TerminalUploadApi, type UploadProgress } from './upload';
const identity = { tabId: '10000000-0000-4000-8000-000000000001', generation: '20000000-0000-4000-8000-000000000001', controlEpoch: 3 };
const uploadId = '30000000-0000-4000-8000-000000000001';
const handle = '40000000-0000-4000-8000-000000000001';
const token = '50000000-0000-4000-8000-000000000001';
const chunkBytes = 192 * 1024;
let data: Buffer;
let current: boolean;
let file: PickedFile;
let progress: UploadProgress[];
let api: { [K in keyof TerminalUploadApi]: ReturnType<typeof vi.fn<TerminalUploadApi[K]>> };
let files: { [K in keyof PickedFiles]: ReturnType<typeof vi.fn<PickedFiles[K]>> };
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
}
function transfer() { return new TerminalUpload({ api, files, identity, token, isCurrent: () => current, onProgress: value => progress.push(value) }); }
beforeEach(() => {
  data = Buffer.alloc(chunkBytes * 2 + 17);
  for (let i = 0; i < data.length; i++) data[i] = i % 256;
  current = true; progress = [];
  file = { handle, fileName: '日本語 image.bin', size: data.length };
  api = {
    beginTerminalUpload: vi.fn(async () => success({ uploadId, chunkBytes })),
    writeTerminalUpload: vi.fn(async input => success({ offset: input.offset + Buffer.from(input.data, 'base64').length })),
    finishTerminalUpload: vi.fn(async () => success({ path: '/host/日本語 image.bin', fileName: file.fileName, size: file.size })),
    cancelTerminalUpload: vi.fn(async () => success({ cancelled: true as const })),
  };
  files = {
    pick: vi.fn(async () => file),
    read: vi.fn(async (_handle, offset, length) => ({ data: data.subarray(offset, offset + length).toString('base64'), bytes: length })),
    release: vi.fn(async () => {}), cancelPick: vi.fn(async () => {}),
  };
});
it('uploads image/document binary unchanged in sequential chunks with identity on every operation', async () => {
  const reply = deferred<Awaited<ReturnType<TerminalUploadApi['writeTerminalUpload']>>>();
  api.writeTerminalUpload.mockImplementationOnce(() => reply.promise);
  const running = transfer().run();
  await vi.waitFor(() => expect(api.writeTerminalUpload).toHaveBeenCalledOnce());
  expect(files.read).toHaveBeenCalledOnce();
  expect(api.finishTerminalUpload).not.toHaveBeenCalled();
  reply.resolve(success({ offset: chunkBytes }));
  expect(await running).toEqual({ path: '/host/日本語 image.bin', fileName: file.fileName, size: data.length });
  const writes = api.writeTerminalUpload.mock.calls.map(([input]) => input);
  expect(writes.map(input => input.offset)).toEqual([0, chunkBytes, chunkBytes * 2]);
  expect(Buffer.concat(writes.map(input => Buffer.from(input.data, 'base64')))).toEqual(data);
  for (const input of writes) expect(input).toMatchObject({ ...identity, uploadId });
  expect(api.beginTerminalUpload).toHaveBeenCalledExactlyOnceWith({ ...identity, fileName: file.fileName, size: file.size });
  expect(api.finishTerminalUpload).toHaveBeenCalledExactlyOnceWith({ ...identity, uploadId });
  expect(api.cancelTerminalUpload).not.toHaveBeenCalled();
  expect(files.release).toHaveBeenCalledExactlyOnceWith(handle);
  expect(progress.at(-1)).toMatchObject({ phase: 'uploading', offset: file.size, size: file.size });
});
it('uploads empty files with begin and finish only', async () => {
  file.size = 0;
  await transfer().run();
  expect(api.beginTerminalUpload).toHaveBeenCalledOnce();
  expect(api.finishTerminalUpload).toHaveBeenCalledOnce();
  expect(files.read).not.toHaveBeenCalled();
  expect(api.writeTerminalUpload).not.toHaveBeenCalled();
});
it('treats picker cancellation as no upload and releases nothing', async () => {
  files.pick.mockResolvedValue(null);
  expect(await transfer().run()).toBeNull();
  expect(api.beginTerminalUpload).not.toHaveBeenCalled();
  expect(files.release).not.toHaveBeenCalled();
});
it('cancels a pending picker and releases a handle returned after cancellation', async () => {
  const picked = deferred<PickedFile | null>();
  files.pick.mockReturnValue(picked.promise);
  const upload = transfer(); const running = upload.run();
  upload.cancel(); picked.resolve(file);
  await expect(running).rejects.toBeInstanceOf(UploadCancelled);
  expect(files.cancelPick).toHaveBeenCalledExactlyOnceWith(token);
  expect(files.release).toHaveBeenCalledExactlyOnceWith(handle);
  expect(api.beginTerminalUpload).not.toHaveBeenCalled();
});
it('cancels a late begin reply after a terminal switch and never sends file data', async () => {
  const begun = deferred<Awaited<ReturnType<TerminalUploadApi['beginTerminalUpload']>>>();
  api.beginTerminalUpload.mockReturnValue(begun.promise);
  const upload = transfer(); const running = upload.run();
  await vi.waitFor(() => expect(api.beginTerminalUpload).toHaveBeenCalledOnce());
  current = false; upload.cancel(); begun.resolve(success({ uploadId, chunkBytes }));
  await expect(running).rejects.toBeInstanceOf(UploadCancelled);
  expect(api.cancelTerminalUpload).toHaveBeenCalledWith({ ...identity, uploadId });
  expect(files.read).not.toHaveBeenCalled();
  expect(files.release).toHaveBeenCalledExactlyOnceWith(handle);
});
it.each(['disconnect', 'ownership epoch change', 'generation change'])('suppresses stale completion after %s', async () => {
  const finished = deferred<Awaited<ReturnType<TerminalUploadApi['finishTerminalUpload']>>>();
  api.finishTerminalUpload.mockReturnValue(finished.promise);
  const running = transfer().run();
  await vi.waitFor(() => expect(api.finishTerminalUpload).toHaveBeenCalledOnce());
  current = false;
  finished.resolve(success({ path: '/host/file', fileName: file.fileName, size: file.size }));
  await expect(running).rejects.toBeInstanceOf(UploadCancelled);
  expect(api.cancelTerminalUpload).toHaveBeenCalledWith({ ...identity, uploadId });
  expect(files.release).toHaveBeenCalledExactlyOnceWith(handle);
});
it('stops after an uncertain write failure, releases local cache, and attempts remote cancellation without retrying data', async () => {
  api.writeTerminalUpload.mockRejectedValueOnce(new Error('Disconnected'));
  api.cancelTerminalUpload.mockRejectedValue(new Error('Disconnected'));
  await expect(transfer().run()).rejects.toThrow('Disconnected');
  expect(api.writeTerminalUpload).toHaveBeenCalledOnce();
  expect(api.finishTerminalUpload).not.toHaveBeenCalled();
  expect(files.release).toHaveBeenCalledExactlyOnceWith(handle);
  expect(api.cancelTerminalUpload).toHaveBeenCalledWith({ ...identity, uploadId });
});
it('shows an explicit old-host upgrade error without retrying or reading the file', async () => {
  api.beginTerminalUpload.mockResolvedValue(failure('VALIDATION', 'Unknown remote operation'));
  await expect(transfer().run()).rejects.toThrow('Upgrade the Shellfox host to attach files.');
  expect(api.beginTerminalUpload).toHaveBeenCalledOnce();
  expect(api.writeTerminalUpload).not.toHaveBeenCalled();
  expect(files.read).not.toHaveBeenCalled();
  expect(files.release).toHaveBeenCalledExactlyOnceWith(handle);
});
it('does not advance on malformed host offsets or truncated native reads', async () => {
  api.writeTerminalUpload.mockResolvedValue(success({ offset: 1 }));
  await expect(transfer().run()).rejects.toThrow('unexpected file offset');
  expect(api.writeTerminalUpload).toHaveBeenCalledOnce();
  expect(api.finishTerminalUpload).not.toHaveBeenCalled();
  expect(api.cancelTerminalUpload).toHaveBeenCalledWith({ ...identity, uploadId });
});
it('stops before writing a truncated native read', async () => {
  files.read.mockResolvedValue({ data: 'AA==', bytes: 1 });
  await expect(transfer().run()).rejects.toThrow('selected file changed');
  expect(api.writeTerminalUpload).not.toHaveBeenCalled();
  expect(files.release).toHaveBeenCalledExactlyOnceWith(handle);
});
it('cancels during an in-flight write and ignores its late success', async () => {
  const written = deferred<Awaited<ReturnType<TerminalUploadApi['writeTerminalUpload']>>>();
  api.writeTerminalUpload.mockReturnValue(written.promise);
  const upload = transfer(); const running = upload.run();
  await vi.waitFor(() => expect(api.writeTerminalUpload).toHaveBeenCalledOnce());
  upload.cancel(); written.resolve(success({ offset: chunkBytes }));
  await expect(running).rejects.toBeInstanceOf(UploadCancelled);
  expect(api.writeTerminalUpload).toHaveBeenCalledOnce();
  expect(api.finishTerminalUpload).not.toHaveBeenCalled();
  expect(files.release).toHaveBeenCalledExactlyOnceWith(handle);
});
