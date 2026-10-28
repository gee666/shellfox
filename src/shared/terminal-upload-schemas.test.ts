import { randomUUID } from 'node:crypto';
import { expect, it } from 'vitest';
import { requestSchemas as localRequestSchemas } from './schemas';
import { remoteForwardRequestSchema, responseSchemas } from './remote-schemas';
import { TERMINAL_UPLOAD_CHUNK_BYTES, uploadRequestSchemas } from './terminal-upload-schemas';

const identity = () => ({ tabId: randomUUID(), generation: randomUUID(), controlEpoch: 1 });
it('keeps upload privileges out of the local Manager IPC contract', () => {
  for (const method of Object.keys(uploadRequestSchemas)) expect(Object.hasOwn(localRequestSchemas, method)).toBe(false);
});
it('requires complete identity and rejects filenames, sizes, offsets and unknown keys outside the contract', () => {
  const begin = { ...identity(), fileName: 'photo.png', size: 0 };
  expect(uploadRequestSchemas.beginTerminalUpload.safeParse(begin).success).toBe(true);
  for (const extra of [{ size: -1 }, { size: 1.5 }, { size: 1024 ** 3 + 1 }, { fileName: '' }, { fileName: 'a'.repeat(1025) }, { controlEpoch: 0 }, { generation: undefined }, { path: '/tmp/escape' }]) {
    expect(uploadRequestSchemas.beginTerminalUpload.safeParse({ ...begin, ...extra }).success).toBe(false);
  }
  const chunk = { ...identity(), uploadId: randomUUID(), offset: 0, data: 'AA==' };
  for (const extra of [{ offset: -1 }, { offset: 0.5 }, { uploadId: 'guess' }, { controlEpoch: undefined }]) expect(uploadRequestSchemas.writeTerminalUpload.safeParse({ ...chunk, ...extra }).success).toBe(false);
});
it('bounds canonical base64 and the complete encoded remote envelope', () => {
  const chunk = { ...identity(), uploadId: randomUUID(), offset: 0, data: Buffer.alloc(TERMINAL_UPLOAD_CHUNK_BYTES, 255).toString('base64') };
  expect(remoteForwardRequestSchema.safeParse({ peerId: 'host', method: 'writeTerminalUpload', payload: chunk }).success).toBe(true);
  expect(JSON.stringify(chunk).length).toBeLessThan(1024 * 1024 - 1024);
  for (const data of ['', 'A', 'AA', 'AA=', 'AB==', 'AAB=', 'AA-_', 'AA==\n', 'AAAA=', '====', Buffer.alloc(TERMINAL_UPLOAD_CHUNK_BYTES + 1).toString('base64')]) {
    expect(uploadRequestSchemas.writeTerminalUpload.safeParse({ ...chunk, data }).success, data.slice(0, 20)).toBe(false);
  }
  for (let length = 1; length <= 256; length++) expect(uploadRequestSchemas.writeTerminalUpload.safeParse({ ...chunk, data: Buffer.alloc(length, length % 256).toString('base64') }).success).toBe(true);
});
it('validates every upload response and does not accept executable or relative path responses', () => {
  expect(responseSchemas.beginTerminalUpload.safeParse({ ok: true, value: { uploadId: randomUUID(), chunkBytes: 1 } }).success).toBe(false);
  expect(responseSchemas.writeTerminalUpload.safeParse({ ok: true, value: { offset: -1 } }).success).toBe(false);
  for (const path of ['../evil', 'file:///tmp/evil', '/tmp/a\ncommand']) expect(responseSchemas.finishTerminalUpload.safeParse({ ok: true, value: { path, fileName: 'a', size: 0 } }).success).toBe(false);
  expect(responseSchemas.cancelTerminalUpload.safeParse({ ok: true, value: { cancelled: false } }).success).toBe(false);
});
