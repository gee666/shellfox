import { failure, type AppError, type Result } from './contracts';
import type { FileApi, FileList, FileLocation, FileRead, FileWriteResult } from './file-contracts';
import { uploadFileChunks } from './file-api';

export type FileOpenResult =
  | { kind: 'file'; file: FileRead }
  | { kind: 'directory'; listing: FileList }
  /** path is the absolute location that was searched, when the host can resolve it. */
  | { kind: 'error'; error: AppError; path: string };

const isAbsolute = (path: string) => /^(?:[/\\]|[a-z]:[\\/])/i.test(path);

/** Joins a host display path and a relative path, keeping the host's separator style. */
export function joinFilePath(base: string, relative: string): string {
  const separator = base.includes('\\') && !base.includes('/') ? '\\' : '/';
  let path = base;
  for (const part of relative.split(/[\\/]+/)) {
    if (!part || part === '.') continue;
    if (part !== '..') { path = path.endsWith(separator) ? path + part : path + separator + part; continue; }
    const index = path.lastIndexOf(separator), root = separator === '/' ? 1 : 3;
    path = index < root ? path.slice(0, root) : path.slice(0, index);
  }
  return path;
}

async function searchedPath(api: Pick<FileApi, 'list'>, location: FileLocation): Promise<string> {
  const { path } = location;
  if (isAbsolute(path)) return path;
  const home = path === '~' || /^~[\\/]/.test(path);
  const base = await api.list({ ...location, path: home ? '~' : '.' }).catch(() => null);
  return base?.ok ? joinFilePath(base.value.path, home ? path.slice(1) : path) : path;
}

/** Terminal links may name files or folders; only a failed read and list is an error. */
export async function openFileTarget(api: Pick<FileApi, 'read' | 'list'>, location: FileLocation): Promise<FileOpenResult> {
  const read = await api.read(location);
  if (read.ok) return { kind: 'file', file: read.value };
  const listed = await api.list(location);
  if (listed.ok) return { kind: 'directory', listing: listed.value };
  return { kind: 'error', error: read.error, path: await searchedPath(api, location) };
}

/** A single file name, never a path, so creation stays in the browsed folder. */
export function fileNameError(name: string): string | null {
  if (!name.trim()) return 'Enter a file name.';
  if (name === '.' || name === '..' || /[\\/\x00-\x1f\x7f]/.test(name)) return 'Enter a file name without folders.';
  if (/[<>:"|?*]/.test(name) || /[. ]$/.test(name)) return 'This name is not valid on every system.';
  return null;
}

/** Creates an empty text file. The host refuses to overwrite an existing one. */
export async function createTextFile(api: FileApi, folder: Omit<FileLocation, 'path'> & { path: string }, name: string): Promise<Result<FileWriteResult>> {
  const invalid = fileNameError(name);
  if (invalid) return failure('VALIDATION', invalid);
  const { beginWrite, writeChunk, finishWrite, cancelTransfer } = api;
  if (!beginWrite || !writeChunk || !finishWrite || !cancelTransfer) return failure('UNSUPPORTED', 'This host cannot create files.');
  return uploadFileChunks({ beginWrite, writeChunk, finishWrite, cancelTransfer },
    { ...folder, path: joinFilePath(folder.path, name), size: 0, mode: 'create', content: 'text' }, async () => new Uint8Array());
}
