import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { failure, success } from '../../../src/shared/contracts';
import { FILE_TEXT_BYTES } from '../../../src/shared/file-schemas';
import { FILE_CHUNK_BYTES } from '../../../src/shared/file-transfer-schemas';
import { RemoteClient } from './client';
import { flush, ids, TestTransport } from './test-fixtures';

let transport: TestTransport;
let client: RemoteClient;
beforeEach(async () => {
  vi.useFakeTimers(); transport = new TestTransport();
  client = new RemoteClient('host-1', transport, { createClientId: () => ids.client });
  client.connect(); await flush();
});
afterEach(() => { client.stop(); vi.useRealTimers(); });
const location = { sessionId: ids.session, tabId: ids.tab, path: './test.txt' };
const revision = 'a'.repeat(64), transferId = '20000000-0000-4000-8000-000000000001';
it('streams mobile file previews and edits through the paired host with shared validation', async () => {
  let read = 'test', size = 0;
  transport.operation = async body => {
    const { method, payload } = JSON.parse(body);
    if (method === 'listFiles') return JSON.stringify(success({ path: '/work', parentPath: '/', entries: [], truncated: false }));
    if (method === 'beginFileRead') {
      read = payload.assetPath ? 'a' : 'test';
      return JSON.stringify(success({ transferId, path: '/work/test.txt', size: read.length, revision, kind: 'text', mimeType: payload.assetPath ? 'text/css' : 'text/plain', chunkBytes: FILE_CHUNK_BYTES }));
    }
    if (method === 'readFileChunk') return JSON.stringify(success({ offset: read.length, data: Buffer.from(read).toString('base64'), eof: true }));
    if (method === 'beginFileWrite') { size = payload.size; return JSON.stringify(success({ transferId, chunkBytes: FILE_CHUNK_BYTES })); }
    if (method === 'writeFileChunk') return JSON.stringify(success({ offset: payload.offset + Buffer.from(payload.data, 'base64').length }));
    if (method === 'finishFileWrite') return JSON.stringify(success({ path: '/work/test.txt', size, revision }));
    return JSON.stringify(success({ cancelled: true }));
  };
  expect(await client.files.list(location)).toMatchObject({ ok: true });
  expect(await client.files.read(location)).toMatchObject({ ok: true, value: { text: 'test' } });
  expect(await client.files.write({ ...location, text: 'saved', revision })).toMatchObject({ ok: true });
  expect(await client.files.readAsset({ ...location, assetPath: 'styles.css' })).toMatchObject({ ok: true });
  expect(await client.files.writeImage({ ...location, dataUrl: 'data:image/png;base64,YQ==', revision })).toMatchObject({ ok: true });
  const methods = transport.calls('/v1/request').map(body => JSON.parse(body).method);
  expect(methods).toContain('beginFileRead'); expect(methods).toContain('writeFileChunk');
  expect(methods).not.toContain('readFile'); expect(methods).not.toContain('writeImageFile');
  expect(JSON.parse(transport.calls('/v1/request')[0]).payload).toEqual(location);
});
it('keeps host conflicts visible and rejects oversized/invalid edits before sending', async () => {
  transport.operation = async () => JSON.stringify(failure('FILE_CONFLICT', 'Changed on host. Reload first.'));
  expect(await client.files.write({ ...location, text: 'saved', revision })).toMatchObject({ ok: false, error: { code: 'FILE_CONFLICT' } });
  expect(await client.files.write({ ...location, text: '\0', revision })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await client.files.write({ ...location, text: 'x'.repeat(FILE_TEXT_BYTES + 1), revision })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await client.files.read({ ...location, path: 'file:///etc/passwd' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(transport.calls('/v1/request')).toHaveLength(1);
  expect(client.getState().status).toBe('connected');
});
it('opens the host home without a running session', async () => {
  transport.operation = async () => JSON.stringify(success({ path: '/home/host', parentPath: '/home', entries: [], truncated: false }));
  expect(await client.files.list({ path: '~' })).toMatchObject({ ok: true });
  expect(JSON.parse(transport.calls('/v1/request')[0]).payload).toEqual({ path: '~' });
});
