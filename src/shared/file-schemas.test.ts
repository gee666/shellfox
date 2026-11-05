import { randomUUID } from 'node:crypto';
import { expect, it } from 'vitest';
import { requestSchemas, responseSchemas } from './schemas';
import { remoteForwardRequestSchema, REMOTE_REQUEST_BYTES } from './remote-schemas';
import { FILE_IMAGE_BYTES, FILE_INLINE_IMAGE_WRITE_BYTES as FILE_IMAGE_WRITE_BYTES, FILE_INLINE_TEXT_BYTES as FILE_TEXT_BYTES } from './file-schemas';

const location = { sessionId: randomUUID(), tabId: randomUUID(), path: 'src/a.ts' };
it('validates narrow file requests and rejects unknown properties and control characters', () => {
  for (const method of ['listFiles', 'readFile'] as const) {
    expect(requestSchemas[method].safeParse(location).success).toBe(true);
    for (const payload of [{ ...location, command: 'rm' }, { ...location, path: 'bad\0path' }, { ...location, sessionId: 'invalid' }, { ...location, path: 'file:///secret' }]) expect(requestSchemas[method].safeParse(payload).success).toBe(false);
  }
  expect(requestSchemas.writeFile.safeParse({ ...location, revision: '0'.repeat(64), text: '雪'.repeat(FILE_TEXT_BYTES) }).success).toBe(false);
  expect(responseSchemas.readFile.safeParse({ ok: true, value: { path: '/a', size: 1, revision: 'invalid', kind: 'text', text: 'a', mimeType: 'text/plain' } }).success).toBe(false);
});
it('rejects inconsistent file payload metadata and active-content image MIME types', () => {
  const metadata = { path: '/image.png', revision: '0'.repeat(64), kind: 'image' as const, mimeType: 'image/png' as const, size: FILE_IMAGE_BYTES, dataUrl: 'data:image/png;base64,' + Buffer.alloc(FILE_IMAGE_BYTES).toString('base64') };
  expect(responseSchemas.readFile.safeParse({ ok: true, value: metadata }).success).toBe(true);
  for (const value of [{ ...metadata, dataUrl: 'data:text/html;base64,YQ==' }, { ...metadata, size: 1 }, { ...metadata, mimeType: 'text/html' }, { kind: 'text', path: '/a', mimeType: 'text/plain', revision: '0'.repeat(64), size: 2, text: 'a' }]) expect(responseSchemas.readFile.safeParse({ ok: true, value }).success).toBe(false);
});
it('fits maximum text/image writes through the existing encoded remote request budget', () => {
  for (const [method, payload] of [
    ['writeFile', { ...location, revision: '0'.repeat(64), text: '\u0001'.repeat(FILE_TEXT_BYTES) }],
    ['writeImageFile', { ...location, revision: '0'.repeat(64), dataUrl: 'data:image/png;base64,' + Buffer.alloc(FILE_IMAGE_WRITE_BYTES).toString('base64') }],
  ]) {
    const request = { peerId: 'host', method, payload };
    expect(remoteForwardRequestSchema.safeParse(request).success).toBe(true);
    expect(Buffer.byteLength(JSON.stringify(request))).toBeLessThan(REMOTE_REQUEST_BYTES - 1024);
  }
});
