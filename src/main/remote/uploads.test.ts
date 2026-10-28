import { randomBytes, randomUUID } from 'node:crypto';
import { lstat, readFile, readdir, writeFile, symlink } from 'node:fs/promises';
import path from 'node:path';
import { expect, it, vi } from 'vitest';
import { responseSchemas } from '../../shared/remote-schemas';
import { TERMINAL_UPLOAD_CHUNK_BYTES } from '../../shared/terminal-upload-schemas';
import { uploadFixture } from './upload-test-fixtures';
import { identity } from './test-fixtures';

it('saves multiple bounded binary chunks exactly and returns a terminal path without writing to the PTY', async () => {
  const f = await uploadFixture(), data = randomBytes(TERMINAL_UPLOAD_CHUNK_BYTES * 2 + 13);
  const upload = await f.begin('../../CON.txt. ', data.length);
  for (let offset = 0; offset < data.length; offset += TERMINAL_UPLOAD_CHUNK_BYTES) {
    const chunk = data.subarray(offset, offset + TERMINAL_UPLOAD_CHUNK_BYTES);
    expect(await f.request('writeTerminalUpload', { ...upload, offset, data: chunk.toString('base64') })).toEqual({ ok: true, value: { offset: offset + chunk.length } });
  }
  const result = responseSchemas.finishTerminalUpload.parse(await f.request('finishTerminalUpload', upload));
  if (!result.ok) throw new Error(result.error.message);
  expect(result.value).toMatchObject({ fileName: 'file_CON.txt', size: data.length });
  expect(path.dirname(result.value.path)).toBe(path.join(f.root, upload.uploadId));
  expect(await readFile(result.value.path)).toEqual(data);
  expect(await readdir(path.dirname(result.value.path))).toEqual(['file_CON.txt']);
  if (process.platform !== 'win32') expect((await lstat(result.value.path)).mode & 0o777).toBe(0o600);
  expect(f.processes[0].write).not.toHaveBeenCalled();
  expect(await f.request('finishTerminalUpload', upload)).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
});
it('preserves an empty file and never overwrites a destination', async () => {
  const f = await uploadFixture(), empty = await f.begin('empty.bin');
  const result = responseSchemas.finishTerminalUpload.parse(await f.request('finishTerminalUpload', empty));
  if (!result.ok) throw new Error('finish failed');
  expect(await readFile(result.value.path)).toEqual(Buffer.alloc(0));
  const duplicate = await f.begin('empty.bin');
  expect(duplicate.uploadId).not.toBe(empty.uploadId);
  const destination = path.join(f.root, duplicate.uploadId, 'empty.bin'); await writeFile(destination, 'existing');
  expect(await f.request('finishTerminalUpload', duplicate)).toMatchObject({ ok: false, error: { code: 'STORAGE_FAILED' } });
  expect(await readFile(result.value.path)).toEqual(Buffer.alloc(0));
  expect(await readdir(f.root)).toEqual([empty.uploadId]);
});
it('rejects unsigned, unapproved, unpaired and other peer/client requests', async () => {
  const f = await uploadFixture(), upload = await f.begin('secret', 1), payload = { ...upload, offset: 0, data: 'AA==' };
  expect(await f.send('/v1/request', { clientId: f.clientId, method: 'writeTerminalUpload', payload }, identity, true)).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  expect(await f.request('writeTerminalUpload', payload, randomUUID())).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  const other = randomUUID(); await f.send('/v1/pair', { clientId: other });
  expect(await f.request('cancelTerminalUpload', upload, other)).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
  const peer = { id: 'other-peer', hostname: 'other' }; await f.send('/v1/pair', { clientId: f.clientId }, peer);
  expect(await f.request('writeTerminalUpload', payload, f.clientId, peer)).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
  f.revoke();
  expect(await f.request('writeTerminalUpload', payload)).toMatchObject({ ok: false, error: { code: 'ACCESS_DENIED' } });
  await vi.waitFor(async () => expect(await readdir(f.root)).toEqual([]));
  f.approve(); const fresh = randomUUID(); await f.send('/v1/pair', { clientId: fresh });
  f.revoke(); expect(await f.request('beginTerminalUpload', { ...f.terminal, fileName: 'a', size: 0 }, fresh)).toMatchObject({ ok: false });
});
it('enforces size, per-client concurrency, offsets, base64 and incomplete finish with partial cleanup', async () => {
  const f = await uploadFixture(10);
  expect(await f.request('beginTerminalUpload', { ...f.terminal, fileName: 'big', size: 11 })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  const one = await f.begin('one', 1), two = await f.begin('two', 1);
  expect(await f.request('beginTerminalUpload', { ...f.terminal, fileName: 'three', size: 1 })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await f.request('writeTerminalUpload', { ...one, offset: 1, data: 'AA==' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await f.request('writeTerminalUpload', { ...two, offset: 0, data: 'AB==' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await readdir(f.root)).toEqual([]);
  const oversize = await f.begin('small', 1);
  expect(await f.request('writeTerminalUpload', { ...oversize, offset: 0, data: 'AAAA' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  const unfinished = await f.begin('partial', 1);
  expect(await f.request('finishTerminalUpload', unfinished)).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await readdir(f.root)).toEqual([]);
});
it('cleans up on cancel, disconnect and terminal generation or control epoch change', async () => {
  const f = await uploadFixture(), first = await f.begin();
  expect(await f.request('cancelTerminalUpload', first)).toEqual({ ok: true, value: { cancelled: true } });
  expect(await readdir(f.root)).toEqual([]);
  await f.begin();
  await f.request('attachTerminal', { tabId: f.terminal.tabId, generation: f.terminal.generation });
  await vi.waitFor(async () => expect(await readdir(f.root)).toEqual([]));
  const attached = responseSchemas.attachTerminal.parse(await f.request('attachTerminal', { tabId: f.terminal.tabId }));
  if (!attached.ok || attached.value.control !== 'owned') throw new Error('attach failed');
  const next = responseSchemas.beginTerminalUpload.parse(await f.request('beginTerminalUpload', { ...f.terminal, controlEpoch: attached.value.controlEpoch, fileName: 'x', size: 1 }));
  if (!next.ok) throw new Error('begin failed');
  const tab = f.repository.tab(f.terminal.tabId); if (!tab) throw new Error('tab missing');
  f.repository.saveTab({ ...tab, operationId: randomUUID() });
  expect(await f.request('writeTerminalUpload', { ...f.terminal, controlEpoch: attached.value.controlEpoch, uploadId: next.value.uploadId, offset: 0, data: 'AA==' })).toMatchObject({ ok: false, error: { code: 'TARGET_LOST' } });
  expect(await readdir(f.root)).toEqual([]);
  f.repository.saveTab(tab);
  const started = await f.request('beginTerminalUpload', { ...f.terminal, controlEpoch: attached.value.controlEpoch, fileName: 'x', size: 1 }); expect(started).toMatchObject({ ok: true });
  await f.send('/v1/disconnect', { clientId: f.clientId });
  await vi.waitFor(async () => expect(await readdir(f.root)).toEqual([]));
});
it('rejects elevated terminals on every upload operation', async () => {
  const f = await uploadFixture(), upload = await f.begin();
  const profiles = f.service.getTerminalProfiles(); if (!profiles.ok) throw new Error('profiles missing');
  vi.spyOn(f.service, 'getTerminalProfiles').mockReturnValue({ ok: true, value: { ...profiles.value, profiles: profiles.value.profiles.map(p => ({ ...p, elevation: 'administrator' })) } });
  for (const method of ['beginTerminalUpload', 'writeTerminalUpload', 'finishTerminalUpload']) {
    const payload = method === 'beginTerminalUpload' ? { ...f.terminal, fileName: 'x', size: 0 } : method === 'writeTerminalUpload' ? { ...upload, offset: 0, data: 'AA==' } : upload;
    expect(await f.request(method, payload)).toMatchObject({ ok: false });
  }
  expect(await readdir(f.root)).toEqual([]);
});
it('does not follow a swapped partial symlink or write outside the inbox', async () => {
  const f = await uploadFixture(), upload = await f.begin('safe', 1), outside = path.join(f.directory, 'outside');
  await writeFile(outside, 'unchanged');
  // A same-user filesystem adversary is stronger than a remote peer, but still must not redirect a write.
  const partial = path.join(f.root, upload.uploadId, '.partial');
  const { unlink } = await import('node:fs/promises'); await unlink(partial);
  try { await symlink(outside, partial); } catch (error) {
    if (process.platform === 'win32' && error instanceof Error && 'code' in error && error.code === 'EPERM') return;
    throw error;
  }
  expect(await f.request('writeTerminalUpload', { ...upload, offset: 0, data: 'AA==' })).toMatchObject({ ok: false, error: { code: 'STORAGE_FAILED' } });
  expect(await readFile(outside, 'utf8')).toBe('unchanged');
  expect(await readdir(f.root)).toEqual([]);
});
