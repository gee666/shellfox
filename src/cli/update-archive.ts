import { execFile } from 'node:child_process';
import { lstat, readdir } from 'node:fs/promises';
import path from 'node:path';
import { promisify } from 'node:util';

const exec = promisify(execFile);
export type ArchiveRun = (file: string, args: string[]) => Promise<string>;
const run: ArchiveRun = async (file, args) => (await exec(file, args, {
  timeout: 120_000, windowsHide: true, maxBuffer: 16 * 1024 * 1024, env: { ...process.env, TAR_OPTIONS: undefined, TAPE: undefined },
})).stdout;

/** Packages contain only regular files and directories. Never extract links or device entries. */
export function validateArchiveListing(names: string, details: string, root: string): void {
  const lines = (value: string) => value.replace(/\r\n/g, '\n').split('\n').filter(Boolean);
  const entries = lines(names), metadata = lines(details), seen = new Set<string>();
  if (!entries.length || entries.length > 50_000 || metadata.length !== entries.length) throw new Error('Invalid headless archive listing.');
  for (const [index, entry] of entries.entries()) {
    const name = entry.replace(/\/$/, '');
    const parts = name.split('/');
    if (parts[0] !== root || parts.some(part => !part || part === '.' || part === '..') ||
      /[\\:\x00-\x1f\x7f]/.test(name) || parts.some(part => /[. ]$/.test(part)) ||
      parts.some(part => /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(part)) ||
      seen.has(name.toLowerCase()) || !/^[-d]/.test(metadata[index])) {
      throw new Error('Unsafe entry in headless archive.');
    }
    seen.add(name.toLowerCase());
  }
}

export async function extractHeadlessArchive(archive: string, destination: string, root: string, platform: string, execute: ArchiveRun = run): Promise<string> {
  const tar = platform === 'win32' ? path.win32.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe') : '/usr/bin/tar';
  const names = await execute(tar, ['-tf', archive]);
  const details = await execute(tar, ['-tvf', archive]);
  validateArchiveListing(names, details, root);
  await execute(tar, ['-xf', archive, '--no-same-owner', '--no-same-permissions', '-C', destination]);
  const installation = path.join(destination, root);
  await validateExtractedTree(installation);
  return installation;
}
async function validateExtractedTree(directory: string): Promise<void> {
  const info = await lstat(directory);
  if (!info.isDirectory() || info.isSymbolicLink()) throw new Error('Invalid headless archive directory.');
  for (const name of await readdir(directory)) {
    const file = path.join(directory, name), entry = await lstat(file);
    if (entry.isSymbolicLink() || (!entry.isDirectory() && !entry.isFile()) || (entry.isFile() && entry.nlink !== 1)) throw new Error('Unsafe extracted headless entry.');
    if (entry.isDirectory()) await validateExtractedTree(file);
  }
}
