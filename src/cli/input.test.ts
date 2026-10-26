import path from 'node:path';
import { chmod, mkdir, mkdtemp, rm, symlink, writeFile } from 'node:fs/promises';
import { Readable } from 'node:stream';
import { afterEach, expect, it } from 'vitest';
import { z } from 'zod';
import { loadJsonArguments } from './input';
import { readAuthKey, readStdin, validateAuthKey } from './secrets';
import { publishPrivateEndpoint } from '../main/control-endpoint';
const roots: string[] = [];
afterEach(async () => { for (const root of roots.splice(0)) await rm(root, { force: true, recursive: true }); });
async function fixture() { await mkdir('tmp', { recursive: true }); const root = await mkdtemp(path.resolve('tmp/cli-input-')); roots.push(root); return root; }
it('loads inline, stdin and file JSON without treating their contents as arguments', async () => {
  const root = await fixture(), file = path.join(root, 'input.json'), value = '{"title":"--json --yes"}';
  await writeFile(file, value);
  expect(await loadJsonArguments(['api', 'renameSession', '--input', value])).toEqual(['api', 'renameSession', '--input', value]);
  expect(await loadJsonArguments(['api', 'renameSession', '--input=-'], Readable.from([value]))).toEqual(['api', 'renameSession', '--input=' + value]);
  expect(await loadJsonArguments(['settings', 'update', '--input', '@' + file])).toEqual(['settings', 'update', '--input', value]);
});
it('does not read stdin after the literal argument separator', async () => {
  expect(await loadJsonArguments(['--', '--input', '-'], Readable.from(['not read']))).toEqual(['--', '--input', '-']);
});
it('bounds and validates UTF-8 stdin', async () => {
  await expect(readStdin(Readable.from(['12345']), 4)).rejects.toThrow('limit');
  await expect(readStdin(Readable.from([Buffer.from([0xff])]))).rejects.toThrow();
});
it('rejects directories and oversized JSON files', async () => {
  const root = await fixture(), file = path.join(root, 'large.json');
  await writeFile(file, 'x'.repeat(1024 * 1024 + 1));
  await expect(loadJsonArguments(['--input', '@' + file])).rejects.toThrow('1 MiB');
  await expect(loadJsonArguments(['--input', '@' + root])).rejects.toThrow();
});
it('reads a protected raw auth key without requiring a JSON credentials file', async () => {
  const root = await fixture(), file = path.join(root, 'private', 'auth-key');
  await publishPrivateEndpoint(root, file, z.object({ secret: z.string() }), { secret: 'placeholder' });
  await writeFile(file, ['tskey', 'auth', 'synthetic-secret'].join('-') + '\n');
  expect(await readAuthKey(file)).toBe(['tskey', 'auth', 'synthetic-secret'].join('-'));
});
it.skipIf(process.platform === 'win32')('rejects auth keys with public permissions or symbolic links', async () => {
  const root = await fixture(), file = path.join(root, 'auth-key'); await writeFile(file, 'secret', { mode: 0o600 });
  const link = path.join(root, 'link'); await symlink(file, link);
  await expect(readAuthKey(link)).rejects.toThrow('not a link');
  await chmod(file, 0o644); await expect(readAuthKey(file)).rejects.toThrow('0600');
});
it.each(['', '  ', 'two keys', 'key\u0000suffix', 'key\nsecond', 'x'.repeat(4097)])('rejects malformed auth key input', value => {
  expect(() => validateAuthKey(value)).toThrow('Invalid auth key');
});
