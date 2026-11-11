import path from 'node:path';
import { mkdir, mkdtemp, realpath, rm, writeFile } from 'node:fs/promises';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { openFileExplorer } from './desktop-explorer';

const native = vi.hoisted(() => ({ openPath: vi.fn(async () => ''), showItemInFolder: vi.fn() }));
vi.mock('electron', () => ({ shell: native }));
const roots: string[] = [];
beforeEach(() => { vi.clearAllMocks(); native.openPath.mockResolvedValue(''); });
afterEach(async () => { for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true }); });
async function fixture() {
  await mkdir('tmp', { recursive: true });
  const root = await mkdtemp(path.resolve('tmp/desktop-explorer-')); roots.push(root);
  const file = path.join(root, 'file with spaces.txt');
  await writeFile(file, 'hello');
  return { root: await realpath(root), file: await realpath(file), snapshot: { sessions: [] } };
}
it('enters directories in the system file manager', async () => {
  const f = await fixture();
  expect(await openFileExplorer({ path: f.root }, f.snapshot)).toEqual({ ok: true, value: null });
  expect(native.openPath).toHaveBeenCalledExactlyOnceWith(f.root);
  expect(native.showItemInFolder).not.toHaveBeenCalled();
});
it('reveals files without launching their associated application', async () => {
  const f = await fixture();
  expect(await openFileExplorer({ path: f.file }, f.snapshot)).toEqual({ ok: true, value: null });
  expect(native.showItemInFolder).toHaveBeenCalledExactlyOnceWith(f.file);
  expect(native.openPath).not.toHaveBeenCalled();
});
it('rejects stale identities and missing paths without native side effects', async () => {
  const f = await fixture();
  expect(await openFileExplorer({ sessionId: crypto.randomUUID(), path: f.file }, f.snapshot)).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
  expect(await openFileExplorer({ path: path.join(f.root, 'missing') }, f.snapshot)).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
  expect(native.openPath).not.toHaveBeenCalled(); expect(native.showItemInFolder).not.toHaveBeenCalled();
});
it('reports file manager failures', async () => {
  const f = await fixture(); native.openPath.mockResolvedValueOnce('No file manager');
  expect(await openFileExplorer({ path: f.root }, f.snapshot)).toMatchObject({ ok: false, error: { code: 'INTERNAL' } });
});
