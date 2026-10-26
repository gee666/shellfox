import path from 'node:path';
import { mkdir, mkdtemp, rm, stat, writeFile } from 'node:fs/promises';
import { afterEach, expect, it } from 'vitest';
import { createDirectory, resolveDirectory } from './directory-prompt';

const directories: string[] = [];
afterEach(async () => { for (const directory of directories.splice(0)) await rm(directory, { recursive: true, force: true }); });
it('creates all parents using the same quoted path normalization as resolveDirectory', async () => {
  await mkdir('tmp', { recursive: true }); const root = await mkdtemp(path.resolve('tmp/create-directory-')); directories.push(root);
  const target = path.join(root, 'one', 'two', 'new folder'), input = '"' + target + '"';
  expect(await resolveDirectory({ path: input })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
  expect(await createDirectory({ path: input })).toEqual({ ok: true, value: { cwd: target } });
  expect((await stat(target)).isDirectory()).toBe(true);
  expect(await resolveDirectory({ path: input })).toEqual({ ok: true, value: { cwd: target } });
  expect(await createDirectory({ path: input })).toEqual({ ok: true, value: { cwd: target } });
}, 10_000);
it('does not offer creation or mutate through an existing file', async () => {
  await mkdir('tmp', { recursive: true }); const root = await mkdtemp(path.resolve('tmp/create-file-')); directories.push(root);
  const file = path.join(root, 'file'); await writeFile(file, 'untouched');
  expect(await resolveDirectory({ path: file })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await resolveDirectory({ path: path.join(file, 'child') })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await createDirectory({ path: path.join(file, 'child') })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect((await stat(file)).isFile()).toBe(true);
});
