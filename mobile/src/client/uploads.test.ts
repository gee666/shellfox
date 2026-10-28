import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { z } from 'zod';
import { failure, success } from '../../../src/shared/contracts';
import { uploadRequestSchemas } from '../../../src/shared/terminal-upload-schemas';
import { TerminalUpload } from '../terminal/upload';
import { RemoteClient } from './client';
import { flush, ids, TestTransport } from './test-fixtures';
const identity = { tabId: ids.tab, generation: ids.generation, controlEpoch: 1 };
const uploadId = '30000000-0000-4000-8000-000000000001';
const token = '40000000-0000-4000-8000-000000000001';
let transport: TestTransport;
let client: RemoteClient;
beforeEach(async () => {
  vi.useFakeTimers(); transport = new TestTransport();
  client = new RemoteClient('host-1', transport, { createClientId: () => ids.client });
  client.connect(); await flush();
});
afterEach(() => { client.stop(); vi.useRealTimers(); });
it('sends attachments only over the approved native request route with shared upload contracts', async () => {
  const bytes = Buffer.from([0, 255, 128, 10, 13]);
  const received: Buffer[] = [];
  transport.operation = async body => {
    const input = z.object({ clientId: z.literal(ids.client), method: z.string(), payload: z.unknown() }).strict().parse(JSON.parse(body));
    switch (input.method) {
      case 'beginTerminalUpload':
        expect(uploadRequestSchemas.beginTerminalUpload.parse(input.payload)).toEqual({ ...identity, fileName: 'image.png', size: bytes.length });
        return JSON.stringify(success({ uploadId, chunkBytes: 196608 }));
      case 'writeTerminalUpload': {
        const chunk = uploadRequestSchemas.writeTerminalUpload.parse(input.payload);
        expect(chunk).toMatchObject({ ...identity, uploadId, offset: 0 });
        received.push(Buffer.from(chunk.data, 'base64'));
        return JSON.stringify(success({ offset: bytes.length }));
      }
      case 'finishTerminalUpload':
        expect(uploadRequestSchemas.finishTerminalUpload.parse(input.payload)).toEqual({ ...identity, uploadId });
        return JSON.stringify(success({ path: '/host/image.png', fileName: 'image.png', size: bytes.length }));
      default: throw new Error('Unexpected upload operation');
    }
  };
  const files = {
    pick: vi.fn(async () => ({ handle: token, fileName: 'image.png', size: bytes.length })),
    read: vi.fn(async () => ({ data: bytes.toString('base64'), bytes: bytes.length })),
    release: vi.fn(async () => {}), cancelPick: vi.fn(async () => {}),
  };
  const result = await new TerminalUpload({ api: client, files, token, identity, isCurrent: () => true, onProgress: () => {} }).run();
  expect(result?.path).toBe('/host/image.png');
  expect(Buffer.concat(received)).toEqual(bytes);
  expect(transport.calls('/v1/request')).toHaveLength(3);
  expect(files.release).toHaveBeenCalledExactlyOnceWith(token);
  expect(client.getState().status).toBe('connected');
});
it.each([
  ['VALIDATION', 'Unknown remote operation'],
  ['AUTH_FAILED', 'This operation is not available remotely.'],
] as const)('does not retry old-host %s upload replies or disconnect an otherwise healthy host', async (code, message) => {
  transport.operation = async () => JSON.stringify(failure(code, message));
  const files = {
    pick: vi.fn(async () => ({ handle: token, fileName: 'file', size: 0 })), read: vi.fn(),
    release: vi.fn(async () => {}), cancelPick: vi.fn(async () => {}),
  };
  await expect(new TerminalUpload({ api: client, files, token, identity, isCurrent: () => true, onProgress: () => {} }).run()).rejects.toThrow('Upgrade the Shellfox host');
  await vi.advanceTimersByTimeAsync(30_000);
  expect(transport.calls('/v1/request')).toHaveLength(1);
  expect(transport.calls('/v1/pair')).toHaveLength(1);
  expect(client.getState().status).toBe('connected');
});
it('still invalidates the connection for genuine upload authentication failures', async () => {
  transport.operation = async () => JSON.stringify(failure('AUTH_FAILED', 'Client expired.'));
  expect(await client.beginTerminalUpload({ ...identity, fileName: 'file', size: 0 })).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  expect(client.getState().status).toBe('reconnecting');
});
