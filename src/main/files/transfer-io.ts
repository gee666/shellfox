import path from 'node:path';
import { constants, type Stats } from 'node:fs';
import { open, stat, realpath } from 'node:fs/promises';
import type { FileHandle } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { failure, success, type Result } from '../../shared/contracts';
import { FILE_CHUNK_BYTES } from '../../shared/file-transfer-schemas';
import { FileError } from './paths';
import { imageMimeType, mimeType, assetMimeType } from './content';
export type FileInfo = Stats;
export const sameFile = (a: FileInfo, b: FileInfo): boolean => a.dev === b.dev && a.ino === b.ino && a.size === b.size && a.mtimeMs === b.mtimeMs && a.ctimeMs === b.ctimeMs;
export async function fileResult<T>(action: () => Promise<T>): Promise<Result<T>> {
  try { return success(await action()); }
  catch (error) {
    if (error instanceof FileError) return failure(error.code, error.message);
    const code = error instanceof Error && 'code' in error ? error.code : null;
    if (code === 'ENOENT' || code === 'ENOTDIR') return failure('NOT_FOUND', 'File or directory not found.');
    if (code === 'EEXIST') return failure('FILE_CONFLICT', 'A file already exists at this destination.');
    if (code === 'EACCES' || code === 'EPERM' || code === 'ELOOP') return failure('ACCESS_DENIED', 'File access was denied.');
    return failure('STORAGE_FAILED', 'File transfer failed. No uncertain operation was retried.', true);
  }
}
export function requireValid(valid: () => boolean): void { if (!valid()) throw new FileError('ACCESS_DENIED', 'File transfer authorization expired or was cancelled.'); }
export async function openRegular(file: string): Promise<FileHandle> {
  const handle = await open(file, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0) | (constants.O_NONBLOCK ?? 0));
  if (!(await handle.stat()).isFile()) { await handle.close(); throw new FileError('UNSUPPORTED', 'Only regular files can be transferred.'); }
  return handle;
}
export async function inspectFile(handle: FileHandle, name: string, valid: () => boolean): Promise<{ revision: string; kind: 'text' | 'image' | 'binary'; mimeType: string }> {
  const info = await handle.stat(), hash = createHash('sha256'), buffer = Buffer.alloc(FILE_CHUNK_BYTES), decoder = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });
  let offset = 0, text = true, image: ReturnType<typeof imageMimeType> = null;
  while (offset < info.size) {
    requireValid(valid);
    const { bytesRead } = await handle.read(buffer, 0, Math.min(buffer.length, info.size - offset), offset);
    if (!bytesRead) throw new FileError('FILE_CONFLICT', 'File changed during transfer.');
    const bytes = buffer.subarray(0, bytesRead); if (!offset) image = imageMimeType(bytes);
    hash.update(bytes);
    if (text) { try { if (bytes.includes(0)) text = false; else decoder.decode(bytes, { stream: true }); } catch { text = false; } }
    offset += bytesRead;
  }
  if (text) { try { decoder.decode(); } catch { text = false; } }
  requireValid(valid);
  if (!sameFile(info, await handle.stat())) throw new FileError('FILE_CONFLICT', 'File changed during transfer.');
  return { revision: hash.digest('hex'), kind: image ? 'image' : text ? 'text' : 'binary', mimeType: image ?? (text ? mimeType(name) : 'application/octet-stream') };
}
export async function assetTarget(base: string, asset: string): Promise<{ file: string; mimeType: string }> {
  if (/^(?:[/\\]|[a-z]:)|[?#]/i.test(asset) || (process.platform === 'win32' && asset.includes(':'))) throw new FileError('ACCESS_DENIED', 'Preview assets must be relative paths without queries or fragments.');
  const html = await realpath(base);
  if (!/\.(?:html?|xhtml|md|mdx|markdown|mdown|mkd)$/i.test(html) || !(await stat(html)).isFile()) throw new FileError('VALIDATION', 'Assets require an existing HTML or Markdown file.');
  const file = await realpath(path.resolve(path.dirname(html), asset)), relative = path.relative(path.dirname(html), file);
  if (relative === '..' || relative.startsWith('..' + path.sep) || path.isAbsolute(relative)) throw new FileError('ACCESS_DENIED', 'Asset is outside the preview directory.');
  return { file, mimeType: assetMimeType(file) };
}
