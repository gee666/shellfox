import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { mkdir, mkdtemp, writeFile, readFile, rm } from 'node:fs/promises';
import { afterEach, expect, it, vi } from 'vitest';
import { FileService } from './service';
import type { FileSessionContext } from './paths';
import { responseSchemas } from '../../shared/schemas';
const cleanup: Array<() => Promise<void>> = [];
afterEach(async () => { for (const close of cleanup.splice(0).reverse()) await close(); });
async function fixture() {
  await mkdir('tmp', { recursive: true }); const root = await mkdtemp(path.resolve('tmp/current-folder-')); cleanup.push(() => rm(root, { recursive: true, force: true }));
  const current = path.join(root, 'after cd'); await mkdir(current); await writeFile(path.join(current, 'note.txt'), 'current file');
  await writeFile(path.join(current, 'page.html'), '<p>preview</p>'); await writeFile(path.join(current, 'style.css'), 'body{}');
  const sessionId = randomUUID(), tabId = randomUUID(), resolveDirectory = vi.fn(async () => current);
  const context: FileSessionContext = { session: () => ({ cwd: root }), tab: () => ({ sessionId, cwd: root }), terminal: () => ({ cwd: root, directoryUrl: null, root: { environment: 'local', distro: null } }), resolveDirectory };
  const service = new FileService(context); cleanup.push(() => service.dispose());
  return { root, current, context, service, resolveDirectory, location: { sessionId, tabId } };
}
it('uses verified current cwd for the contextual button and relative reads, saves and preview assets', async () => {
  const f = await fixture();
  expect(await f.service.list({ ...f.location, path: '.' })).toMatchObject({ ok: true, value: { path: f.current, entries: expect.arrayContaining([expect.objectContaining({ name: 'note.txt' })]) } });
  const read = await f.service.read({ ...f.location, path: 'note.txt' }); if (!read.ok) throw new Error(read.error.message);
  expect(read.value).toMatchObject({ kind: 'text', text: 'current file' });
  expect(await f.service.write({ ...f.location, path: 'note.txt', revision: read.value.revision, text: 'saved in current directory' })).toMatchObject({ ok: true });
  expect(await readFile(path.join(f.current, 'note.txt'), 'utf8')).toBe('saved in current directory');
  expect(await f.service.readAsset({ ...f.location, path: 'page.html', assetPath: 'style.css' })).toMatchObject({ ok: true, value: { mimeType: 'text/css' } });
  const begin = responseSchemas.beginFileWrite.parse(await f.service.transfers.request({ id: 'view', valid: () => true }, 'beginFileWrite', { ...f.location, path: 'empty.txt', mode: 'create', content: 'file', size: 0 }));
  if (!begin.ok) throw new Error(begin.error.message);
  expect(await f.service.transfers.request({ id: 'view', valid: () => true }, 'finishFileWrite', { transferId: begin.value.transferId })).toMatchObject({ ok: true, value: { path: path.join(f.current, 'empty.txt') } });
});
it('does not claim launch cwd as current when an old Windows shell cannot report it', async () => {
  const f = await fixture(); f.resolveDirectory.mockRejectedValue(new Error('Reopen the PowerShell tab to enable directory tracking.'));
  expect(await f.service.list({ ...f.location, path: '.' })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED', message: expect.stringContaining('Reopen') } });
});
it('keeps absolute browsing, reads, saves and assets independent of live terminal directory queries', async () => {
  const f = await fixture(); f.resolveDirectory.mockRejectedValue(new Error('Current directory unavailable.'));
  const file = path.join(f.current, 'note.txt');
  expect((await f.service.list({ ...f.location, path: f.current })).ok).toBe(true);
  const read = await f.service.read({ ...f.location, path: file }); if (!read.ok) throw new Error(read.error.message);
  expect((await f.service.write({ ...f.location, path: file, revision: read.value.revision, text: 'absolute save' })).ok).toBe(true);
  expect((await f.service.readAsset({ ...f.location, path: path.join(f.current, 'page.html'), assetPath: 'style.css' })).ok).toBe(true);
  const begin = responseSchemas.beginFileRead.parse(await f.service.transfers.request({ id: 'view', valid: () => true }, 'beginFileRead', { ...f.location, path: file }));
  expect(begin.ok).toBe(true); expect(f.resolveDirectory).not.toHaveBeenCalled();
});
it('rejects a foreign OSC7 hostname before any native cwd lookup can reinterpret it locally', async () => {
  const f = await fixture(), service = new FileService({ ...f.context, terminal: () => ({ cwd: f.root, directoryUrl: 'file://different-host.invalid/work', root: { environment: 'local', distro: null } }) }); cleanup.push(() => service.dispose());
  expect(await service.list({ ...f.location, path: '.' })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED', message: expect.stringContaining('different host') } });
  expect(f.resolveDirectory).not.toHaveBeenCalled();
});
