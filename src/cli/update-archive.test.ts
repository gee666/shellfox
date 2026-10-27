import { execFile } from 'node:child_process';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { promisify } from 'node:util';
import path from 'node:path';
import { afterEach, expect, it, vi } from 'vitest';
import { extractHeadlessArchive, validateArchiveListing } from './update-archive';
const root = 'shellfox-headless-linux-x64';
const cleanup: string[] = [];
afterEach(async () => { for (const directory of cleanup.splice(0)) await rm(directory, { recursive: true, force: true }); });
it('accepts ordinary file and directory listings from GNU tar and bsdtar', () => {
  for (const newline of ['\n', '\r\n']) {
    expect(() => validateArchiveListing([root + '/', root + '/app/', root + '/app/main.cjs', ''].join(newline), ['drwxr-xr-x user/group 0', 'drwxr-xr-x user/group 0', '-rw-r--r-- user/group 42', ''].join(newline), root)).not.toThrow();
  }
});
it.each([
  [root + '/../outside', '-'], ['../outside', '-'], ['/absolute', '-'], ['C:/absolute', '-'],
  [root + '/app\\..\\outside', '-'], [root + '/app/./file', '-'], [root + '//file', '-'],
  [root + '/file:stream', '-'], [root + '/file.', '-'], [root + '/file ', '-'], [root + '/CON', '-'], [root + '/nul.txt', '-'],
  [root + '/link', 'l'], [root + '/hardlink', 'h'], [root + '/device', 'c'], [root + '/pipe', 'p'],
  ['other-platform/app/main.cjs', '-'], [root + '/control\x1bfile', '-'],
])('rejects unsafe archive entry %j type %s', (name, type) => {
  expect(() => validateArchiveListing(name + '\n', type + ' metadata\n', root)).toThrow();
});
it('rejects duplicates, case collisions, missing entries, and empty archives', () => {
  expect(() => validateArchiveListing('', '', root)).toThrow();
  expect(() => validateArchiveListing(root + '/file\n', '', root)).toThrow();
  for (const second of ['file', 'FILE']) {
    expect(() => validateArchiveListing(`${root}/file\n${root}/${second}\n`, '- file\n- file\n', root)).toThrow();
  }
});
it.each(['win32', 'darwin', 'linux'])('validates listings before calling the %s system extractor', async platform => {
  await mkdir('tmp', { recursive: true });
  const directory = await mkdtemp(path.resolve('tmp/archive update ')); cleanup.push(directory);
  const execute = vi.fn(async (_file: string, args: string[]) => {
    if (args[0] === '-tf') return root + '/app/main.cjs\n';
    if (args[0] === '-tvf') return '-rw-r--r-- user/group 42\n';
    await mkdir(path.join(directory, root, 'app'), { recursive: true });
    await writeFile(path.join(directory, root, 'app/main.cjs'), 'mock program, never executed');
    return '';
  });
  expect(await extractHeadlessArchive('archive with spaces', directory, root, platform, execute)).toBe(path.join(directory, root));
  expect(execute.mock.calls.map(([, args]) => args[0])).toEqual(['-tf', '-tvf', '-xf']);
  expect(execute.mock.calls[0][0]).toBe(platform === 'win32' ? path.win32.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/tar.exe') : '/usr/bin/tar');
  expect(execute.mock.calls[2][1]).toEqual(['-xf', 'archive with spaces', '--no-same-owner', '--no-same-permissions', '-C', directory]);
});
for (const format of [process.platform === 'win32' ? 'zip' : 'tar.gz']) {
  it(`extracts a real local ${format} fixture with the native system tools`, async () => {
  await mkdir('tmp', { recursive: true });
  const directory = await mkdtemp(path.resolve('tmp/native archive update ')); cleanup.push(directory);
  const source = path.join(directory, 'source'), destination = path.join(directory, 'destination');
  const packageRoot = `shellfox-headless-${process.platform}-${process.arch}`;
  await mkdir(path.join(source, packageRoot, 'app'), { recursive: true }); await mkdir(destination);
  await writeFile(path.join(source, packageRoot, 'app/main.cjs'), 'local fixture, never executed');
  const archive = path.join(directory, 'release.' + format), exec = promisify(execFile);
  if (format === 'tar.gz') {
    const tar = process.platform === 'win32' ? path.win32.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/tar.exe') : '/usr/bin/tar';
    await exec(tar, ['-czf', archive, '-C', source, packageRoot], { timeout: 10_000 });
  } else {
    const quote = (value: string) => "'" + value.replaceAll("'", "''") + "'";
    await exec('powershell.exe', ['-NoLogo', '-NoProfile', '-NonInteractive', '-Command',
      `$ErrorActionPreference='Stop'; Compress-Archive -LiteralPath ${quote(path.join(source, packageRoot))} -DestinationPath ${quote(archive)}`,
    ], { timeout: 15_000 });
  }
  const installed = await extractHeadlessArchive(archive, destination, packageRoot, process.platform);
  expect(await readFile(path.join(installed, 'app/main.cjs'), 'utf8')).toBe('local fixture, never executed');
  }, 20_000);
}
it('does not extract an archive with links', async () => {
  const execute = vi.fn(async (_file: string, args: string[]) => args[0] === '-tf' ? root + '/link\n' : 'lrwxrwxrwx link -> /outside\n');
  await expect(extractHeadlessArchive('archive', 'unused', root, 'linux', execute)).rejects.toThrow('Unsafe entry');
  expect(execute).toHaveBeenCalledTimes(2);
});
