import { readFileSync } from 'node:fs';
import path from 'node:path';
import { createContext, runInContext } from 'node:vm';
import { afterEach, expect, it, vi } from 'vitest';
import { decodeFileUtf8, isWellFormedFileText } from './file-utf8';
import { createFileApi, encodeFileChunk, type FileWire } from './file-api';
import { FILE_TEXT_BYTES, fileEditorWriteSchema } from './file-schemas';
import { FILE_CHUNK_BYTES } from './file-transfer-schemas';
import { success } from './contracts';

const encoder = new TextEncoder(), revision = 'a'.repeat(64), transferId = '10000000-0000-4000-8000-000000000001';
afterEach(() => vi.unstubAllGlobals());
function polyfilledRuntime() {
  const context = createContext({ Buffer: undefined, TextDecoder: undefined, TextEncoder: undefined });
  runInContext(readFileSync(path.resolve('node_modules/fast-text-encoding/text.min.js'), 'utf8'), context);
  return context;
}
function fileWire(bytes: Uint8Array) {
  const beginWrite = vi.fn(async () => success({ transferId, chunkBytes: FILE_CHUNK_BYTES }));
  const cancelTransfer = vi.fn(async () => success({ cancelled: true as const }));
  const wire: FileWire = {
    list: async () => success({ path: '/host', parentPath: '/', entries: [], truncated: false }),
    beginRead: async () => success({ transferId, path: '/host/text.txt', size: bytes.length, revision, kind: 'text', mimeType: 'text/plain', chunkBytes: FILE_CHUNK_BYTES }),
    readChunk: async ({ offset }) => { const part = bytes.subarray(offset, offset + FILE_CHUNK_BYTES); return success({ offset: offset + part.length, data: encodeFileChunk(part), eof: offset + part.length === bytes.length }); },
    beginWrite, writeChunk: async ({ offset, data }) => success({ offset: offset + atob(data).length }),
    finishWrite: async () => success({ path: '/host/text.txt', revision, size: bytes.length }), cancelTransfer,
  };
  return { wire, beginWrite, cancelTransfer };
}
it('strictly decodes Unicode, CRLF, NUL and BOM without stripping or replacing bytes', () => {
  for (const text of ['', 'ASCII\r\n', '\ufeffBOM\r\n雪🙂', 'a\0b', '\u007f\u0080\u07ff\u0800\ud7ff\ue000\uffff', String.fromCodePoint(0x10000, 0x10ffff)]) expect(decodeFileUtf8(encoder.encode(text))).toBe(text);
  expect(decodeFileUtf8(encoder.encode('x'.repeat(FILE_TEXT_BYTES)))).toHaveLength(FILE_TEXT_BYTES);
});
it.each([
  [0x80], [0xbf], [0xc0, 0x80], [0xc1, 0xbf], [0xc2], [0xc2, 0x20],
  [0xe0, 0x80, 0x80], [0xe0, 0x9f, 0xbf], [0xed, 0xa0, 0x80], [0xed, 0xbf, 0xbf],
  [0xe2, 0x82], [0xe2, 0x20, 0xac], [0xe2, 0x82, 0x20],
  [0xf0, 0x80, 0x80, 0x80], [0xf0, 0x8f, 0xbf, 0xbf], [0xf4, 0x90, 0x80, 0x80],
  [0xf0, 0x9f, 0x99], [0xf0, 0x9f, 0x20, 0x80], [0xf5, 0x80, 0x80, 0x80], [0xfe], [0xff],
])('rejects malformed UTF-8 case %# instead of replacing it', (...bytes) => {
  expect(() => decodeFileUtf8(Uint8Array.from(bytes))).toThrow('valid UTF-8');
});
it('reproduces installed Hermes-like polyfill fatal failure but reads real streamed text successfully', async () => {
  const context = polyfilledRuntime();
  expect(() => runInContext("new TextDecoder('utf-8', {fatal:true, ignoreBOM:true})", context)).toThrow('fatal');
  const nativeEncoder: unknown = runInContext('TextEncoder', context), unsupportedDecoder: unknown = runInContext('TextDecoder', context);
  expect(typeof nativeEncoder).toBe('function'); expect(typeof unsupportedDecoder).toBe('function');
  vi.stubGlobal('TextEncoder', nativeEncoder); vi.stubGlobal('TextDecoder', unsupportedDecoder);
  const text = '\ufeffWindows host\r\n雪🙂', f = fileWire(encoder.encode(text));
  expect(await createFileApi(f.wire).read({ path: '/host/text.txt' })).toMatchObject({ ok: true, value: { kind: 'text', text, size: encoder.encode(text).length } });
  expect(f.cancelTransfer).toHaveBeenCalledWith({ transferId });
  const invalid = fileWire(Uint8Array.from([0xed, 0xa0, 0x80]));
  expect(await createFileApi(invalid.wire).read({ path: '/host/text.txt' })).toMatchObject({ ok: false, error: { code: 'INTERNAL' } });
  const nul = fileWire(encoder.encode('a\0b'));
  expect(await createFileApi(nul.wire).read({ path: '/host/text.txt' })).toMatchObject({ ok: false, error: { code: 'INTERNAL' } });
});
it('rejects ill-formed UTF-16 before a polyfill encoder can drop it or start a write', async () => {
  const context = polyfilledRuntime();
  expect(runInContext("Array.from(new TextEncoder().encode('\\ud800'))", context)).toEqual([]);
  const nativeEncoder: unknown = runInContext('TextEncoder', context); vi.stubGlobal('TextEncoder', nativeEncoder);
  const f = fileWire(encoder.encode('original')), api = createFileApi(f.wire);
  for (const text of ['\ud800', '\udfff', 'before\ud800after', '\ud800\ud800', '\udc00\ud800']) {
    expect(isWellFormedFileText(text)).toBe(false);
    expect(fileEditorWriteSchema.safeParse({ path: '/host/text.txt', revision, text }).success).toBe(false);
    expect(await api.write({ path: '/host/text.txt', revision, text })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  }
  expect(f.beginWrite).not.toHaveBeenCalled();
  expect(isWellFormedFileText('valid🙂\ufeff雪')).toBe(true);
  expect(await api.write({ path: '/host/text.txt', revision, text: 'valid🙂\ufeff雪' })).toMatchObject({ ok: true });
});
