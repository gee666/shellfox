import { readdir } from 'node:fs/promises';
import { afterEach, expect, it, vi } from 'vitest';
import { responseSchemas } from '../../shared/remote-schemas';
import { uploadFixture } from './upload-test-fixtures';
import * as uploadPath from './upload-path';
import { UploadStorage } from './upload-storage';
import { TerminalUploads } from './uploads';

afterEach(() => vi.restoreAllMocks());
it('revalidates approval after asynchronous path translation and removes every partial on revoke', async () => {
  const f = await uploadFixture(), upload = await f.begin('empty');
  let complete: (value: string) => void = () => {};
  const path = new Promise<string>(resolve => { complete = resolve; });
  const resolve = vi.spyOn(uploadPath, 'terminalUploadPath').mockReturnValue(path);
  const finished = f.request('finishTerminalUpload', upload);
  await vi.waitFor(() => expect(resolve).toHaveBeenCalledOnce());
  f.revoke(); complete('/mnt/c/not-published');
  expect(await finished).toMatchObject({ ok: false, error: { code: 'ACCESS_DENIED' } });
  expect(await readdir(f.root)).toEqual([]);
});
it('fences a changed control epoch after async writes and rejects simultaneous chunks', async () => {
  const f = await uploadFixture(), upload = await f.begin('one', 1);
  let complete: () => void = () => {};
  const pending = new Promise<void>(resolve => { complete = resolve; });
  const verify = vi.spyOn(UploadStorage.prototype, 'verify').mockImplementationOnce(async () => pending);
  const written = f.request('writeTerminalUpload', { ...upload, offset: 0, data: 'AA==' });
  await vi.waitFor(() => expect(verify).toHaveBeenCalled());
  expect(await f.request('writeTerminalUpload', { ...upload, offset: 0, data: 'AA==' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  await f.request('attachTerminal', { tabId: upload.tabId, generation: upload.generation });
  complete();
  expect(await written).toMatchObject({ ok: false, error: { code: 'TERMINAL_CONTROL_REQUIRED' } });
  expect(await readdir(f.root)).toEqual([]);
});
it('cleans up a partial when final storage publication fails', async () => {
  const f = await uploadFixture(), upload = await f.begin('one');
  vi.spyOn(UploadStorage.prototype, 'save').mockRejectedValueOnce(new Error('disk failed'));
  expect(await f.request('finishTerminalUpload', upload)).toMatchObject({ ok: false, error: { code: 'STORAGE_FAILED' } });
  expect(await readdir(f.root)).toEqual([]);
});
it('expires inactive partials and allows the initiator to cancel after losing control', async () => {
  const f = await uploadFixture(), delivery = f.service.createTerminalView(() => {});
  const terminal = { tabId: f.terminal.tabId, generation: f.terminal.generation };
  delivery.attach(terminal); delivery.take(terminal);
  const attached = delivery.attach(terminal);
  if (!attached.ok || attached.value.control !== 'owned') throw new Error('owner missing');
  const identity = { ...f.terminal, controlEpoch: attached.value.controlEpoch };
  const uploads = new TerminalUploads(f.service, { root: f.root }), actor = { id: 'direct', delivery, valid: () => true };
  try {
    const first = responseSchemas.beginTerminalUpload.parse(await uploads.request(actor, 'beginTerminalUpload', { ...identity, fileName: 'expire', size: 1 }));
    if (!first.ok) throw new Error('begin failed');
    const now = Date.now(); vi.spyOn(Date, 'now').mockReturnValue(now + 5 * 60_000 + 1);
    await uploads.sweep(); expect(await readdir(f.root)).toEqual([]); vi.restoreAllMocks();
    const next = responseSchemas.beginTerminalUpload.parse(await uploads.request(actor, 'beginTerminalUpload', { ...identity, fileName: 'cancel', size: 1 }));
    if (!next.ok) throw new Error('begin failed');
    delivery.attach(terminal);
    expect(await uploads.request(actor, 'cancelTerminalUpload', { ...identity, uploadId: next.value.uploadId })).toEqual({ ok: true, value: { cancelled: true } });
    expect(await readdir(f.root)).toEqual([]);
  } finally { await uploads.dispose(); delivery.dispose(); }
});
