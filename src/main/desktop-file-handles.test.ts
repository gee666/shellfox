import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mkdir, mkdtemp, readFile, readdir, rename, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { DesktopFileHandles } from './desktop-file-handles';
import { FILE_CHUNK_BYTES, FILE_TRANSFER_TTL_MS } from '../shared/file-transfer-schemas';
import type { Result } from '../shared/contracts';

const value = <T>(result: Result<T>): T => { if (!result.ok) throw new Error(result.error.message); return result.value; };
let directory: string;
const services: DesktopFileHandles[] = [];
const service = (upload: string | null = null, download: string | null = null) => {
  const local = new DesktopFileHandles({ upload: async () => upload, download: async () => download });
  services.push(local); return local;
};
beforeEach(async () => { await mkdir('tmp', { recursive: true }); directory = await mkdtemp(path.resolve('tmp/desktop-file-handles-')); });
afterEach(async () => { await Promise.all(services.splice(0).map(local => local.dispose())); vi.useRealTimers(); await rm(directory, { recursive: true, force: true }); });

describe('local upload handles', () => {
  it('reads bounded sequential chunks with next offsets and no exposed paths', async () => {
    const file = path.join(directory, 'source.bin'), bytes = Buffer.alloc(FILE_CHUNK_BYTES + 13, 41);
    await writeFile(file, bytes); const local = service(file);
    const picked = value(await local.pickUpload()); expect(picked).not.toBeNull(); if (!picked) return;
    expect(picked).toEqual({ handle: expect.any(String), name: 'source.bin', size: bytes.length, chunkBytes: FILE_CHUNK_BYTES });
    expect(JSON.stringify(picked)).not.toContain(directory);
    const first = value(await local.readUpload({ handle: picked.handle, offset: 0 }));
    expect(first.offset).toBe(FILE_CHUNK_BYTES); expect(first.eof).toBe(false); expect(Buffer.from(first.data, 'base64')).toEqual(bytes.subarray(0, FILE_CHUNK_BYTES));
    expect(await local.readUpload({ handle: picked.handle, offset: 0 })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    const last = value(await local.readUpload({ handle: picked.handle, offset: first.offset }));
    expect(last.offset).toBe(bytes.length); expect(last.eof).toBe(true);
    value(await local.releaseUpload({ handle: picked.handle }));
    expect(await local.readUpload({ handle: picked.handle, offset: bytes.length })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
  });
  it('supports empty files and native picker cancellation', async () => {
    expect(value(await service().pickUpload())).toBeNull();
    const file = path.join(directory, 'empty'); await writeFile(file, ''); const local = service(file), picked = value(await local.pickUpload());
    if (!picked) throw new Error('No upload handle');
    expect(value(await local.readUpload({ handle: picked.handle, offset: 0 }))).toEqual({ offset: 0, data: '', eof: true });
  });
  it('does not share handles between owner documents and rejects changed sources', async () => {
    const file = path.join(directory, 'source'); await writeFile(file, 'before');
    const first = service(file), second = service(file), picked = value(await first.pickUpload());
    if (!picked) throw new Error('No upload handle');
    expect(await second.readUpload({ handle: picked.handle, offset: 0 })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
    await writeFile(file, 'changed size');
    expect(await first.readUpload({ handle: picked.handle, offset: 0 })).toMatchObject({ ok: false, error: { code: 'FILE_CONFLICT' } });
  });
  it('revokes a picker still open during owner disposal', async () => {
    const file = path.join(directory, 'source'); await writeFile(file, 'data');
    let choose: (file: string) => void = () => {};
    const local = new DesktopFileHandles({ upload: () => new Promise(resolve => { choose = resolve; }), download: async () => null }); services.push(local);
    const request = local.pickUpload(); await local.dispose(); choose(file);
    expect(await request).toMatchObject({ ok: false, error: { code: 'ACCESS_DENIED' } });
  });
  it('bounds handle count and expires inactive leases', async () => {
    vi.useFakeTimers(); const file = path.join(directory, 'source'); await writeFile(file, 'x'); const local = service(file);
    const handles: string[] = [];
    for (let index = 0; index < 8; index++) { const picked = value(await local.pickUpload()); if (picked) handles.push(picked.handle); }
    expect(await local.pickUpload()).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    await vi.advanceTimersByTimeAsync(FILE_TRANSFER_TTL_MS + 30_000);
    expect(await local.readUpload({ handle: handles[0], offset: 0 })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
    expect(value(await local.pickUpload())).not.toBeNull();
  });
});

describe('staged local downloads', () => {
  it('writes sequential chunks and publishes only after complete commit', async () => {
    const target = path.join(directory, 'download.bin'), local = service(null, target);
    const selected = value(await local.beginDownload({ name: 'remote.bin', size: 6 })); if (!selected) throw new Error('No download handle');
    expect(await readFile(target).catch(() => null)).toBeNull();
    expect(await local.writeDownload({ handle: selected.handle, offset: 1, data: 'YWJj' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    expect(value(await local.writeDownload({ handle: selected.handle, offset: 0, data: 'YWJj' }))).toEqual({ offset: 3 });
    expect(await local.finishDownload(selected)).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    value(await local.writeDownload({ handle: selected.handle, offset: 3, data: 'ZGVm' }));
    value(await local.finishDownload(selected)); expect(await readFile(target, 'utf8')).toBe('abcdef');
    expect((await readdir(directory)).filter(name => name.startsWith('.shellfox-download-'))).toEqual([]);
  });
  it('commits zero-byte downloads without receiving a chunk', async () => {
    const target = path.join(directory, 'empty'), local = service(null, target), selected = value(await local.beginDownload({ name: 'empty', size: 0 }));
    if (!selected) throw new Error('No download handle'); value(await local.finishDownload(selected)); expect(await readFile(target)).toHaveLength(0);
  });
  it('never clobbers a destination created after the chooser returned', async () => {
    const target = path.join(directory, 'target'), local = service(null, target), selected = value(await local.beginDownload({ name: 'target', size: 1 }));
    if (!selected) throw new Error('No download handle'); value(await local.writeDownload({ ...selected, offset: 0, data: 'YQ==' }));
    await writeFile(target, 'external'); expect(await local.finishDownload(selected)).toMatchObject({ ok: false, error: { code: 'FILE_CONFLICT' } });
    value(await local.cancelDownload(selected)); expect(await readFile(target, 'utf8')).toBe('external');
  });
  it('replaces only the unchanged existing file confirmed through the chooser', async () => {
    const target = path.join(directory, 'target'); await writeFile(target, 'before');
    const local = service(null, target), selected = value(await local.beginDownload({ name: 'target', size: 1 }));
    if (!selected) throw new Error('No download handle'); value(await local.writeDownload({ ...selected, offset: 0, data: 'Yg==' }));
    value(await local.finishDownload(selected)); expect(await readFile(target, 'utf8')).toBe('b');
    const second = value(await local.beginDownload({ name: 'target', size: 0 })); if (!second) throw new Error('No download handle');
    await writeFile(target, 'changed externally'); expect(await local.finishDownload(second)).toMatchObject({ ok: false, error: { code: 'FILE_CONFLICT' } });
    expect(await readFile(target, 'utf8')).toBe('changed externally');
  });
  it('cancels and disposes staged writers without touching the destination', async () => {
    const target = path.join(directory, 'target'), local = service(null, target), selected = value(await local.beginDownload({ name: 'target', size: 2 }));
    if (!selected) throw new Error('No download handle'); value(await local.writeDownload({ ...selected, offset: 0, data: 'YQ==' }));
    await local.dispose(); expect(await readFile(target).catch(() => null)).toBeNull(); expect(await readdir(directory)).toEqual([]);
    expect(await local.writeDownload({ ...selected, offset: 1, data: 'Yg==' })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
  });
  it('refuses a staged file substituted by another process before commit', async () => {
    const target = path.join(directory, 'target'), local = service(null, target), selected = value(await local.beginDownload({ name: 'target', size: 0 }));
    if (!selected) throw new Error('No download handle');
    const stage = path.join(directory, (await readdir(directory))[0]);
    await rename(stage, stage + '.original'); await writeFile(stage, 'substituted');
    expect(await local.finishDownload(selected)).toMatchObject({ ok: false, error: { code: 'FILE_CONFLICT' } });
    expect(await readFile(target).catch(() => null)).toBeNull();
  });
  it('rejects oversized/noncanonical chunks without changing the stream offset', async () => {
    const target = path.join(directory, 'target'), local = service(null, target), selected = value(await local.beginDownload({ name: 'target', size: FILE_CHUNK_BYTES + 1 }));
    if (!selected) throw new Error('No download handle');
    for (const data of ['YR==', Buffer.alloc(FILE_CHUNK_BYTES + 1).toString('base64')]) expect(await local.writeDownload({ ...selected, offset: 0, data })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    expect(value(await local.writeDownload({ ...selected, offset: 0, data: 'YQ==' }))).toEqual({ offset: 1 });
  });
});
