import { createHash, randomUUID } from 'node:crypto';
import { constants } from 'node:fs';
import { copyFile, lstat, mkdir, open, rename, rm } from 'node:fs/promises';
import path from 'node:path';

export interface HelperLaunch { executable: string; args: string[] }
export interface StableHelperOptions {
  candidate: string;
  expectedSha256: string;
  localAppData: string;
  arch: string;
}

export function windowsHelperPath(localAppData: string, arch: string): string {
  if (!path.isAbsolute(localAppData)) throw new Error('Windows helper requires an absolute LOCALAPPDATA directory.');
  if (arch !== 'x64' && arch !== 'arm64') throw new Error('Unsupported Windows helper architecture.');
  return path.join(localAppData, 'Shellfox', 'helpers', 'win32-' + arch, 'shellfox-tailscale.exe');
}

function validateDigest(expected: string): void {
  if (!/^[a-f0-9]{64}$/.test(expected)) throw new Error('Windows helper requires a trusted build SHA256.');
}
async function digest(file: string): Promise<string> {
  const stat = await lstat(file);
  if (!stat.isFile() || stat.isSymbolicLink()) throw new Error('Windows helper must be a regular file.');
  const handle = await open(file, 'r');
  try {
    const hash = createHash('sha256');
    for await (const chunk of handle.createReadStream({ autoClose: false })) hash.update(chunk);
    return hash.digest('hex');
  } finally { await handle.close(); }
}
async function matches(file: string, expected: string): Promise<boolean> {
  try { return await digest(file) === expected; }
  catch (error) {
    if (error instanceof Error && 'code' in error && error.code === 'ENOENT') return false;
    throw error;
  }
}

/** Explicit old helper fixtures retain their original CLI and path, never enter the shared cache. */
export async function pinnedLegacyHelper(executable: string, expectedSha256: string): Promise<HelperLaunch> {
  validateDigest(expectedSha256);
  if (!path.isAbsolute(executable)) throw new Error('Explicit Windows helper requires an absolute path.');
  if (!await matches(executable, expectedSha256)) throw new Error('Explicit Windows helper SHA256 mismatch.');
  return { executable, args: [] };
}

/** Windows network execution uses a stable path. Packaged helpers remain candidates. */
export async function prepareWindowsHelper(options: StableHelperOptions): Promise<HelperLaunch> {
  const expected = options.expectedSha256;
  validateDigest(expected);
  const executable = windowsHelperPath(options.localAppData, options.arch);
  const launch = { executable, args: ['--expected-sha256', expected] };
  if (await matches(executable, expected)) return launch;
  if (path.resolve(options.candidate) === path.resolve(executable)) throw new Error('Windows helper update candidate must be separate from its execution path.');
  if (!await matches(options.candidate, expected)) throw new Error('Windows helper candidate SHA256 mismatch.');
  await mkdir(path.dirname(executable), { recursive: true, mode: 0o700 });
  const staged = path.join(path.dirname(executable), '.shellfox-tailscale-' + randomUUID() + '.tmp');
  let ownsStage = false;
  try {
    await copyFile(options.candidate, staged, constants.COPYFILE_EXCL);
    ownsStage = true;
    if (!await matches(staged, expected)) throw new Error('Staged Windows helper SHA256 mismatch.');
    // Reuse another publisher's matching result. Otherwise Windows denies rename
    // while any network helper holds its actual-executable no-write/delete lease.
    if (!await matches(executable, expected)) {
      try { await rename(staged, executable); }
      catch (cause) {
        throw new Error('Windows helper update could not replace the stable executable. Stop all Shellfox remote helpers and retry. The packaged candidate was retained; no alternate executable will be launched.', { cause });
      }
    }
    if (!await matches(executable, expected)) throw new Error('Windows helper changed during publication. Retry after other Shellfox hosts stop.');
    // The helper verifies this external digest under its actual-image read lease
    // before network effects, closing the publication-to-spawn version race.
    return launch;
  } finally { if (ownsStage) await rm(staged, { force: true }); }
}
