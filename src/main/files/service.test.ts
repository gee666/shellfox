import path from 'node:path';
import { hostname } from 'node:os';
import { randomUUID } from 'node:crypto';
import { mkdir, mkdtemp, readFile, writeFile, readdir, rm, symlink } from 'node:fs/promises';
import { afterEach, expect, it } from 'vitest';
import type { Result } from '../../shared/contracts';
import { FILE_INLINE_PREVIEW_BYTES as FILE_IMAGE_BYTES, FILE_INLINE_IMAGE_WRITE_BYTES as FILE_IMAGE_WRITE_BYTES, FILE_LIST_LIMIT, FILE_INLINE_TEXT_BYTES as FILE_TEXT_BYTES } from '../../shared/file-schemas';
import { FileService } from './service';
import { revision } from './content';

const cleanups: string[] = [];
afterEach(async () => { for (const folder of cleanups.splice(0)) await rm(folder, { recursive: true, force: true }); });
const value = <T>(result: Result<T>): T => { if (!result.ok) throw new Error(result.error.message); return result.value; };
async function fixture() {
  await mkdir('tmp', { recursive: true });
  const root = await mkdtemp(path.resolve('tmp/files-')); cleanups.push(root);
  const sessionId = randomUUID(), tabId = randomUUID();
  const terminal = { cwd: root, directoryUrl: null as string | null, root: { environment: 'local' as const, distro: null } };
  const service = new FileService({ session: id => id === sessionId ? { cwd: root } : undefined, tab: id => id === tabId ? { sessionId, cwd: root } : undefined, terminal: id => id === tabId ? terminal : undefined });
  const input = { sessionId, tabId, path: 'a.txt' };
  await writeFile(path.join(root, 'a.txt'), 'old\r\n雪\ufeff');
  return { root, sessionId, tabId, terminal, service, input };
}
const png = (suffix = '') => Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), Buffer.from(suffix)]);
it('reads exact UTF-8/BOM/line endings and saves with content revisions without stale overwrites', async () => {
  const f = await fixture(), read = value(await f.service.read(f.input));
  expect(read).toMatchObject({ kind: 'text', text: 'old\r\n雪\ufeff', path: path.join(f.root, 'a.txt') });
  const saved = value(await f.service.write({ ...f.input, text: '\ufeffnew\r\n雪', revision: read.revision }));
  expect(await readFile(saved.path, 'utf8')).toBe('\ufeffnew\r\n雪');
  expect(saved.revision).not.toBe(read.revision);
  expect(await f.service.write({ ...f.input, text: 'stale', revision: read.revision })).toMatchObject({ ok: false, error: { code: 'FILE_CONFLICT' } });
  expect((await readdir(f.root)).some(name => name.startsWith('.shellfox-save-'))).toBe(false);
});
it('serializes competing views and checks authorization again before an atomic save', async () => {
  const f = await fixture(), read = value(await f.service.read(f.input));
  const saves = await Promise.all([f.service.write({ ...f.input, revision: read.revision, text: 'first' }), f.service.write({ ...f.input, revision: read.revision, text: 'second' })]);
  expect(saves.filter(result => result.ok)).toHaveLength(1);
  expect(saves.filter(result => !result.ok)).toMatchObject([{ error: { code: 'FILE_CONFLICT' } }]);
  const next = value(await f.service.read(f.input));
  expect(await f.service.write({ ...f.input, revision: next.revision, text: 'unauthorized' }, () => false)).toMatchObject({ ok: false, error: { code: 'ACCESS_DENIED' } });
  expect(await readFile(path.join(f.root, 'a.txt'), 'utf8')).toBe('first');
  expect(await readdir(f.root)).toEqual(['a.txt']);
});
it('bounds lists and reports kinds without following links or recursively enumerating', async () => {
  const f = await fixture(); await mkdir(path.join(f.root, 'folder'));
  const list = value(await f.service.list({ ...f.input, path: '.' }));
  expect(list.entries.map(entry => [entry.name, entry.kind])).toEqual([['folder', 'directory'], ['a.txt', 'file']]);
  await Promise.all(Array.from({ length: FILE_LIST_LIMIT }, (_, i) => writeFile(path.join(f.root, `file-${i}`), '')));
  const bounded = value(await f.service.list({ ...f.input, path: '.' }));
  expect(bounded.entries).toHaveLength(FILE_LIST_LIMIT); expect(bounded.truncated).toBe(true);
});
it('validates the session/tab and rejects oversized, binary, invalid UTF-8 and over-limit writes', async () => {
  const f = await fixture();
  expect(await f.service.read({ ...f.input, sessionId: randomUUID() })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
  expect(await f.service.read({ ...f.input, tabId: randomUUID() })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
  for (const bytes of [Buffer.from([0, 1]), Buffer.from([255, 254]), Buffer.alloc(FILE_TEXT_BYTES + 1, 97)]) {
    await writeFile(path.join(f.root, 'a.txt'), bytes);
    expect(value(await f.service.read(f.input)).kind).toBe('binary');
    expect(await f.service.write({ ...f.input, revision: revision(bytes), text: 'no' })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
  }
  expect(await f.service.write({ ...f.input, revision: '0'.repeat(64), text: '雪'.repeat(FILE_TEXT_BYTES) })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  await writeFile(path.join(f.root, 'a.txt'), Buffer.alloc(FILE_IMAGE_BYTES + 1));
  expect(await f.service.read(f.input)).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
  expect(await f.service.read({ ...f.input, path: '.' })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
});
it('uses current OSC7 cwd, falls back to launch cwd, and never maps a foreign host to the local filesystem', async () => {
  const f = await fixture(); await mkdir(path.join(f.root, 'next')); await writeFile(path.join(f.root, 'next', 'a.txt'), 'current');
  const guest = path.join(f.root, 'next').replace(/\\/g, '/');
  f.terminal.directoryUrl = `file://${hostname()}/${guest.replace(/^\//, '')}`;
  expect(value(await f.service.read(f.input))).toMatchObject({ kind: 'text', text: 'current' });
  f.terminal.directoryUrl = 'file://not-the-local-host.invalid/tmp';
  expect(await f.service.read(f.input)).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
});
it('returns raster metadata and narrowly saves matching PNG/JPEG/WebP images with conflict checks', async () => {
  const f = await fixture(), file = path.join(f.root, 'photo.png'), initial = png('before'), next = png('after');
  await writeFile(file, initial);
  const location = { ...f.input, path: file }, image = value(await f.service.read(location));
  expect(image).toMatchObject({ kind: 'image', mimeType: 'image/png', dataUrl: `data:image/png;base64,${initial.toString('base64')}` });
  const input = { ...location, revision: image.revision, dataUrl: `data:image/png;base64,${next.toString('base64')}` };
  expect(value(await f.service.writeImage(input))).toMatchObject({ size: next.length, revision: revision(next) });
  expect(await readFile(file)).toEqual(next);
  expect(await f.service.writeImage(input)).toMatchObject({ ok: false, error: { code: 'FILE_CONFLICT' } });
  for (const dataUrl of ['data:image/jpeg;base64,' + next.toString('base64'), 'data:image/png;base64,' + Buffer.from('not png').toString('base64'), 'data:application/octet-stream;base64,YQ==', 'data:image/png;base64,!', 'data:image/png;base64,' + png('a'.repeat(FILE_IMAGE_WRITE_BYTES)).toString('base64')]) expect(await f.service.writeImage({ ...input, dataUrl })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
});
it('keeps SVG source editable and reports its preview MIME type', async () => {
  const f = await fixture(), file = path.join(f.root, 'diagram.svg');
  await writeFile(file, '<svg xmlns="http://www.w3.org/2000/svg"><circle r="3"/></svg>');
  const image = value(await f.service.read({ ...f.input, path: file }));
  expect(image).toMatchObject({ kind: 'text', mimeType: 'image/svg+xml' });
  expect(await f.service.write({ ...f.input, path: file, revision: image.revision, text: '<svg/>' })).toMatchObject({ ok: true });
});
it('saves JPEG/WebP only under their matching extensions and rejects cross-format replacements', async () => {
  const f = await fixture();
  for (const [extension, mime, bytes] of [['jpg', 'image/jpeg', Buffer.from([255, 216, 255, 1])], ['webp', 'image/webp', Buffer.from('RIFF0000WEBPcontent')]] as const) {
    const file = path.join(f.root, `picture.${extension}`); await writeFile(file, bytes);
    const location = { ...f.input, path: file }, before = value(await f.service.read(location));
    expect(before).toMatchObject({ kind: 'image', mimeType: mime });
    expect(await f.service.writeImage({ ...location, revision: before.revision, dataUrl: `data:${mime};base64,${bytes.toString('base64')}` })).toMatchObject({ ok: true });
    expect(await f.service.writeImage({ ...location, revision: before.revision, dataUrl: `data:image/png;base64,${png().toString('base64')}` })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  }
});
it('serves sibling HTML/Markdown assets and rejects traversal, unsupported types and symlink escapes', async () => {
  const f = await fixture(); await mkdir(path.join(f.root, 'docs')); await mkdir(path.join(f.root, 'outside'));
  await writeFile(path.join(f.root, 'outside', 'secret.css'), 'secret');
  await writeFile(path.join(f.root, 'docs', 'style.css'), 'body{color:red}');
  await writeFile(path.join(f.root, 'docs', 'script.js'), 'window.demo = true');
  for (const base of ['page.html', 'page.xhtml', 'README.md', 'README.mdx', 'README.markdown', 'README.mdown', 'README.mkd']) {
    await writeFile(path.join(f.root, 'docs', base), 'preview');
    const input = { ...f.input, path: path.join(f.root, 'docs', base) };
    expect(value(await f.service.readAsset({ ...input, assetPath: 'style.css' }))).toMatchObject({ mimeType: 'text/css', dataUrl: 'data:text/css;base64,' + Buffer.from('body{color:red}').toString('base64') });
    expect(value(await f.service.readAsset({ ...input, assetPath: 'script.js' }))).toMatchObject({ mimeType: 'text/javascript' });
    for (const assetPath of ['../outside/secret.css', '/etc/passwd', 'https://example.com/a.css', 'style.css?secret', 'style.css#fragment']) expect((await f.service.readAsset({ ...input, assetPath })).ok).toBe(false);
  }
  await symlink(path.join(f.root, 'outside'), path.join(f.root, 'docs', 'linked'), process.platform === 'win32' ? 'junction' : 'dir');
  expect(await f.service.readAsset({ ...f.input, path: path.join(f.root, 'docs', 'page.html'), assetPath: 'linked/secret.css' })).toMatchObject({ ok: false, error: { code: 'ACCESS_DENIED' } });
});
