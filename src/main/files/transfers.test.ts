import path from 'node:path';
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, writeFile, readFile, readdir, rm, open, symlink } from 'node:fs/promises';
import { afterEach, expect, it } from 'vitest';
import type { Result } from '../../shared/contracts';
import { resultSchema } from '../../shared/schemas';
import { FILE_CHUNK_BYTES, FILE_TRANSFER_TTL_MS, fileTransferValueSchemas, type FileTransferMethod } from '../../shared/file-transfer-schemas';
import type { FileTransferInput, FileTransferValue } from '../../shared/file-transfer-contracts';
import { createFileApi, uploadFileChunks, downloadFileChunks, encodeFileChunk, type FileWire } from '../../shared/file-api';
import { FileService } from './service';
import { FileTransfers, type FileActor } from './transfers';

const cleanup: Array<() => Promise<void>> = [];
afterEach(async () => { for (const close of cleanup.splice(0).reverse()) await close(); });
const value = <T>(result: Result<T>): T => { if (!result.ok) throw new Error(result.error.message); return result.value; };
async function fixture(maxBytes?: number) {
  await mkdir('tmp', { recursive: true }); const root = await mkdtemp(path.resolve('tmp/file-stream-')); cleanup.push(() => rm(root, { recursive: true, force: true }));
  const context = { session: () => undefined, tab: () => undefined }, service = new FileService(context); cleanup.push(() => service.dispose());
  let now = 1000, approved = true;
  const transfers = new FileTransfers(context, { maxBytes, now: () => now }); cleanup.push(() => transfers.dispose());
  const actor: FileActor = { id: 'owner', valid: () => approved };
  function send<M extends FileTransferMethod>(method: M, input: FileTransferInput<M>, owner?: FileActor): Promise<Result<FileTransferValue<M>>>;
  async function send(method: FileTransferMethod, input: FileTransferInput<FileTransferMethod>, owner = actor): Promise<Result<FileTransferValue<FileTransferMethod>>> { return resultSchema(fileTransferValueSchemas[method]).parse(await transfers.request(owner, method, input)); }
  const wire: FileWire = { list: input => service.list(input), beginRead: input => send('beginFileRead', input), readChunk: input => send('readFileChunk', input), beginWrite: input => send('beginFileWrite', input), writeChunk: input => send('writeFileChunk', input), finishWrite: input => send('finishFileWrite', input), cancelTransfer: input => send('cancelFileTransfer', input) };
  return { root, send, transfers, wire, api: createFileApi(wire), actor, expire: () => { now += FILE_TRANSFER_TTL_MS + 1; }, revoke: () => { approved = false; } };
}
it('supports zero-byte files and cancellation before any remote handle is opened', async () => {
  const f = await fixture(), file = path.join(f.root, 'empty.txt');
  const upload = await uploadFileChunks(f.wire, { path: file, mode: 'create', size: 0, content: 'file' }, async () => { throw new Error('No chunks expected'); });
  expect(upload).toMatchObject({ ok: true, value: { size: 0 } });
  const begin = value(await f.send('beginFileRead', { path: file }));
  expect(await f.send('readFileChunk', { transferId: begin.transferId, offset: 0 })).toEqual({ ok: true, value: { offset: 0, data: '', eof: true } });
  await f.send('cancelFileTransfer', { transferId: begin.transferId });
  expect(await downloadFileChunks(f.wire, { path: file }, async () => { throw new Error('Empty files must not write a zero-length chunk'); })).toMatchObject({ ok: true, value: { size: 0 } });
  const controller = new AbortController(); controller.abort();
  expect(await uploadFileChunks(f.wire, { path: path.join(f.root, 'cancelled'), mode: 'create', size: 0, content: 'file' }, async () => new Uint8Array(), { signal: controller.signal })).toMatchObject({ ok: false });
  expect(await downloadFileChunks(f.wire, { path: file }, async () => {}, { signal: controller.signal })).toMatchObject({ ok: false });
  expect(await readdir(f.root)).toEqual(['empty.txt']);
});
it('streams uploads, previews and downloads with no sessions, keeping chunks within the existing transport budget', async () => {
  const f = await fixture(), file = path.join(f.root, 'large.txt'), bytes = Buffer.alloc(3 * 1024 * 1024, 97);
  const saved = value(await uploadFileChunks(f.wire, { path: file, mode: 'create', content: 'file', size: bytes.length }, async (offset, length) => bytes.subarray(offset, offset + length)));
  expect(saved.revision).toBe(createHash('sha256').update(bytes).digest('hex'));
  expect(value(await f.api.read({ path: file }))).toMatchObject({ kind: 'text', size: bytes.length, text: bytes.toString() });
  let downloaded = 0;
  expect((await downloadFileChunks(f.wire, { path: file }, async (part, offset) => { expect(offset).toBe(downloaded); expect(part.length).toBeLessThanOrEqual(FILE_CHUNK_BYTES); downloaded += part.length; })).ok).toBe(true);
  expect(downloaded).toBe(bytes.length);
  expect(await readdir(f.root)).toEqual(['large.txt']);
});
it('previews images above 2 MiB and replaces edited images above the old 512 KiB save ceiling', async () => {
  const f = await fixture(), file = path.join(f.root, 'photo.png'), header = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), original = Buffer.concat([header, Buffer.alloc(3 * 1024 * 1024)]);
  await writeFile(file, original);
  const read = value(await f.api.read({ path: file })); expect(read.kind).toBe('image');
  const edited = Buffer.concat([header, Buffer.alloc(700 * 1024, 1)]);
  expect(await f.api.writeImage({ path: file, revision: read.revision, dataUrl: `data:image/png;base64,${edited.toString('base64')}` })).toMatchObject({ ok: true, value: { size: edited.length } });
  expect(await readFile(file)).toEqual(edited);
});
it('keeps large binary metadata downloadable and reports oversized text as a preview limit, not binary', async () => {
  const f = await fixture(), binary = path.join(f.root, 'large.bin'), handle = await open(binary, 'wx'); await handle.truncate(64 * 1024 * 1024 + 1); await handle.close();
  expect(value(await f.api.read({ path: binary }))).toMatchObject({ kind: 'binary', size: 64 * 1024 * 1024 + 1 });
  const text = path.join(f.root, 'large.txt'); await writeFile(text, Buffer.alloc(8 * 1024 * 1024 + 1, 97));
  expect(await f.api.read({ path: text })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED', message: expect.stringContaining('8 MiB') } });
});
it('binds handles to their initiating owner, rejects bad offsets and deletes partials on cancellation or expiry', async () => {
  const f = await fixture(), pathName = path.join(f.root, 'created.bin');
  const first = value(await f.send('beginFileWrite', { path: pathName, size: 2, mode: 'create', content: 'file' }));
  expect(await f.send('writeFileChunk', { transferId: first.transferId, offset: 0, data: 'YQ==' }, { id: 'other', valid: () => true })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
  expect(await f.send('writeFileChunk', { transferId: first.transferId, offset: 1, data: 'YQ==' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await readdir(f.root)).toEqual([]);
  const next = value(await f.send('beginFileWrite', { path: pathName, size: 0, mode: 'create', content: 'file' }));
  f.expire(); await f.transfers.sweep();
  expect(await f.send('finishFileWrite', { transferId: next.transferId })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
  expect(await readdir(f.root)).toEqual([]);
});
it('enforces upload quotas and concurrent handle limits before opening more staging files', async () => {
  const f = await fixture(1024), input = { path: path.join(f.root, 'one'), size: 1024, mode: 'create' as const, content: 'file' as const };
  const first = value(await f.send('beginFileWrite', input));
  const second = value(await f.send('beginFileWrite', { ...input, path: path.join(f.root, 'two') }));
  expect(await f.send('beginFileWrite', { ...input, path: path.join(f.root, 'three') })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await f.send('beginFileWrite', { ...input, path: path.join(f.root, 'other') }, { id: 'other', valid: () => true })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  await f.send('cancelFileTransfer', { transferId: first.transferId }); await f.send('cancelFileTransfer', { transferId: second.transferId });
  expect(await readdir(f.root)).toEqual([]);
});
it('publishes create uploads without clobbering concurrent files and rejects stale replacements', async () => {
  const f = await fixture(), file = path.join(f.root, 'a.txt');
  const creating = value(await f.send('beginFileWrite', { path: file, size: 1, mode: 'create', content: 'file' }));
  await f.send('writeFileChunk', { transferId: creating.transferId, offset: 0, data: 'YQ==' }); await writeFile(file, 'other');
  expect(await f.send('finishFileWrite', { transferId: creating.transferId })).toMatchObject({ ok: false, error: { code: 'FILE_CONFLICT' } }); expect(await readFile(file, 'utf8')).toBe('other');
  const before = value(await f.api.read({ path: file }));
  const replacing = value(await f.send('beginFileWrite', { path: file, size: 1, mode: 'replace', revision: before.revision, content: 'text' }));
  await f.send('writeFileChunk', { transferId: replacing.transferId, offset: 0, data: 'YQ==' }); await writeFile(file, 'changed');
  expect(await f.send('finishFileWrite', { transferId: replacing.transferId })).toMatchObject({ ok: false, error: { code: 'FILE_CONFLICT' } }); expect(await readFile(file, 'utf8')).toBe('changed');
  expect((await readdir(f.root)).filter(name => name.startsWith('.shellfox-'))).toEqual([]);
});
it('revocation and actor teardown cancel uploads without requiring a running terminal', async () => {
  const f = await fixture(), file = path.join(f.root, 'a.txt');
  const transfer = value(await f.send('beginFileWrite', { path: file, size: 1, mode: 'create', content: 'file' }));
  await f.send('writeFileChunk', { transferId: transfer.transferId, offset: 0, data: 'YQ==' }); f.revoke(); await f.transfers.sweep();
  expect((await f.send('finishFileWrite', { transferId: transfer.transferId })).ok).toBe(false); expect(await readdir(f.root)).toEqual([]);
});
it('confines streaming assets through canonical paths and detects source changes during download', async () => {
  const f = await fixture(), html = path.join(f.root, 'page.html'), css = path.join(f.root, 'style.css'); await writeFile(html, '<p>preview</p>'); await writeFile(css, 'body{}');
  expect(value(await f.api.readAsset({ path: html, assetPath: 'style.css' }))).toMatchObject({ mimeType: 'text/css' });
  expect((await f.send('beginFileRead', { path: html, assetPath: '../outside.css' })).ok).toBe(false);
  const begun = value(await f.send('beginFileRead', { path: css })); await writeFile(css, 'changed');
  expect(await f.send('readFileChunk', { transferId: begun.transferId, offset: 0 })).toMatchObject({ ok: false, error: { code: 'FILE_CONFLICT' } });
  const outer = await mkdtemp(path.resolve('tmp/file-outside-')); cleanup.push(() => rm(outer, { recursive: true, force: true })); await writeFile(path.join(outer, 'secret.css'), 'secret');
  await symlink(outer, path.join(f.root, 'linked'), process.platform === 'win32' ? 'junction' : 'dir');
  expect(await f.send('beginFileRead', { path: html, assetPath: 'linked/secret.css' })).toMatchObject({ ok: false, error: { code: 'ACCESS_DENIED' } });
});
