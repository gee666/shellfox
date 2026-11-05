import path from 'node:path';
import { writeFile, readFile, readdir } from 'node:fs/promises';
import { afterEach, expect, it, vi } from 'vitest';
import { createFileApi, uploadFileChunks, downloadFileChunks, type FileWire } from '../../shared/file-api';
import { responseSchemas } from '../../shared/schemas';
import { FakeRemoteHelper, remoteFixture } from './test-fixtures';

const cleanups: Array<() => Promise<void>> = [];
it('streams file-browser transfers without sessions, requires approval, never clobbers names, and cancels staged writes on revoke', async () => {
  const network = new Map<string, FakeRemoteHelper>();
  const host = await remoteFixture(network, 'host'), client = await remoteFixture(network, 'client'); cleanups.push(host.dispose, client.dispose);
  const file = path.join(host.directory, 'browser-upload.bin');
  await client.remote.connect({ peerId: 'host' });
  await vi.waitFor(() => expect(client.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ connection: 'awaiting-approval' })] } }));
  expect(await client.remote.request({ peerId: 'host', method: 'beginFileWrite', payload: { path: file, size: 0, mode: 'create', content: 'file' } })).toMatchObject({ ok: false, error: { code: 'APPROVAL_REQUIRED' } });
  await host.remote.approve({ peerId: 'client' });
  await vi.waitFor(() => expect(client.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ connection: 'connected' })] } }), { timeout: 5000 });
  const wire: FileWire = {
    list: async input => responseSchemas.listFiles.parse(await client.remote.request({ peerId: 'host', method: 'listFiles', payload: input })),
    beginRead: async input => responseSchemas.beginFileRead.parse(await client.remote.request({ peerId: 'host', method: 'beginFileRead', payload: input })),
    readChunk: async input => responseSchemas.readFileChunk.parse(await client.remote.request({ peerId: 'host', method: 'readFileChunk', payload: input })),
    beginWrite: async input => responseSchemas.beginFileWrite.parse(await client.remote.request({ peerId: 'host', method: 'beginFileWrite', payload: input })),
    writeChunk: async input => responseSchemas.writeFileChunk.parse(await client.remote.request({ peerId: 'host', method: 'writeFileChunk', payload: input })),
    finishWrite: async input => responseSchemas.finishFileWrite.parse(await client.remote.request({ peerId: 'host', method: 'finishFileWrite', payload: input })),
    cancelTransfer: async input => responseSchemas.cancelFileTransfer.parse(await client.remote.request({ peerId: 'host', method: 'cancelFileTransfer', payload: input })),
  };
  const bytes = Buffer.alloc(1024 * 1024, 97), input = { path: file, size: bytes.length, mode: 'create' as const, content: 'file' as const };
  expect(await uploadFileChunks(wire, input, async (offset, length) => bytes.subarray(offset, offset + length))).toMatchObject({ ok: true });
  expect(await readFile(file)).toEqual(bytes);
  expect(await uploadFileChunks(wire, input, async (offset, length) => bytes.subarray(offset, offset + length))).toMatchObject({ ok: false, error: { code: 'FILE_CONFLICT' } });
  expect(await createFileApi(wire).read({ path: file })).toMatchObject({ ok: true, value: { kind: 'text', size: bytes.length } });
  let downloaded = 0; expect(await downloadFileChunks(wire, { path: file }, async part => { downloaded += part.length; })).toMatchObject({ ok: true }); expect(downloaded).toBe(bytes.length);
  const starting = await wire.beginWrite({ ...input, path: path.join(host.directory, 'revoked.bin') }); if (!starting.ok) throw new Error('No transfer');
  await host.remote.revoke({ peerId: 'client' });
  expect(await wire.finishWrite({ transferId: starting.value.transferId })).toMatchObject({ ok: false });
  await vi.waitFor(async () => expect((await readdir(host.directory)).filter(name => name.startsWith('.shellfox-transfer-'))).toEqual([]));
});
afterEach(async () => { for (const close of cleanups.splice(0).reverse()) await close(); });
it('requires peer approval and routes bounded file read/list/save/assets to the host, including conflicts', async () => {
  const network = new Map<string, FakeRemoteHelper>();
  const host = await remoteFixture(network, 'host'), client = await remoteFixture(network, 'client'); cleanups.push(host.dispose, client.dispose);
  const session = await host.create(); if (!session.ok) throw new Error('Session missing');
  const file = path.join(host.directory, 'report.html'), style = path.join(host.directory, 'style.css');
  await writeFile(file, '<h1>host file</h1>'); await writeFile(style, 'body{color:red}');
  const payload = { sessionId: session.value.id, tabId: session.value.tabs[0].id, path: file };
  await client.remote.connect({ peerId: 'host' });
  await vi.waitFor(() => expect(client.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ connection: 'awaiting-approval' })] } }));
  expect(await client.remote.request({ peerId: 'host', method: 'readFile', payload })).toMatchObject({ ok: false, error: { code: 'APPROVAL_REQUIRED' } });
  await host.remote.approve({ peerId: 'client' });
  await vi.waitFor(() => expect(client.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ connection: 'connected' })] } }), { timeout: 5000 });
  const read = responseSchemas.readFile.parse(await client.remote.request({ peerId: 'host', method: 'readFile', payload }));
  if (!read.ok) throw new Error(read.error.message);
  expect(read.value).toMatchObject({ kind: 'text', text: '<h1>host file</h1>' });
  expect(await client.remote.request({ peerId: 'host', method: 'listFiles', payload: { ...payload, path: host.directory } })).toMatchObject({ ok: true, value: { entries: expect.arrayContaining([expect.objectContaining({ name: 'report.html', kind: 'file' })]) } });
  expect(await client.remote.request({ peerId: 'host', method: 'readFileAsset', payload: { ...payload, assetPath: 'style.css' } })).toMatchObject({ ok: true, value: { mimeType: 'text/css' } });
  const save = { ...payload, text: '<h1>saved remotely</h1>', revision: read.value.revision };
  expect(await client.remote.request({ peerId: 'host', method: 'writeFile', payload: save })).toMatchObject({ ok: true });
  expect(await readFile(file, 'utf8')).toBe(save.text);
  expect(await client.remote.request({ peerId: 'host', method: 'writeFile', payload: save })).toMatchObject({ ok: false, error: { code: 'FILE_CONFLICT' } });
  const png = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10, 1]), image = path.join(host.directory, 'image.png');
  await writeFile(image, png);
  const imageRead = responseSchemas.readFile.parse(await client.remote.request({ peerId: 'host', method: 'readFile', payload: { ...payload, path: image } }));
  if (!imageRead.ok) throw new Error('Image missing');
  expect(await client.remote.request({ peerId: 'host', method: 'writeImageFile', payload: { ...payload, path: image, revision: imageRead.value.revision, dataUrl: `data:image/png;base64,${Buffer.concat([png, Buffer.from([2])]).toString('base64')}` } })).toMatchObject({ ok: true });
  await host.remote.revoke({ peerId: 'client' });
  expect(await client.remote.request({ peerId: 'host', method: 'readFile', payload })).toMatchObject({ ok: false });
});
