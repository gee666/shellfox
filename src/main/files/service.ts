import path from 'node:path';
import { FileTransfers } from './transfers';
import { constants } from 'node:fs';
import { open, opendir, realpath, stat, rename, unlink } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import { failure, success, type Result } from '../../shared/contracts';
import type { FileApi, FileLocation, FileWriteInput, FileImageWriteInput, FileAssetInput, FileList, FileRead, FileWriteResult, FileAsset } from '../../shared/file-contracts';
import { FILE_INLINE_PREVIEW_BYTES as FILE_IMAGE_BYTES, FILE_LIST_LIMIT, FILE_LIST_BYTES, fileWriteSchema, fileImageWriteSchema, fileAssetRequestSchema } from '../../shared/file-schemas';
import { FileError, resolveFileTarget, type FileSessionContext } from './paths';
import { revision, mimeType, assetMimeType, imageMimeType, decodeText, decodeImage, dataUrl } from './content';

// Serialize saves across renderer, headless and remote views of the same host.
const writes = new Map<string, Promise<unknown>>();
async function boundedRead(file: string): Promise<Buffer> {
  const handle = await open(file, constants.O_RDONLY | (constants.O_NONBLOCK ?? 0) | (constants.O_NOFOLLOW ?? 0));
  try {
    const info = await handle.stat();
    if (!info.isFile()) throw new FileError('UNSUPPORTED', 'Only regular files can be previewed.');
    if (info.size > FILE_IMAGE_BYTES) throw new FileError('UNSUPPORTED', 'File exceeds the 2 MiB preview limit.');
    // Do not trust a prior stat: another process can grow the file during this read.
    const bytes = Buffer.alloc(Math.min(info.size + 1, FILE_IMAGE_BYTES + 1));
    let offset = 0;
    while (offset < bytes.length) {
      const read = await handle.read(bytes, offset, bytes.length - offset, offset);
      if (!read.bytesRead) break;
      offset += read.bytesRead;
    }
    if (offset > info.size) throw new FileError('FILE_CONFLICT', 'File changed while being read. Retry the read.');
    return bytes.subarray(0, offset);
  } finally { await handle.close(); }
}
const sameFile = (a: Awaited<ReturnType<typeof stat>>, b: Awaited<ReturnType<typeof stat>>) => a.dev === b.dev && a.ino === b.ino && a.size === b.size && a.mtimeMs === b.mtimeMs && a.ctimeMs === b.ctimeMs;
async function result<T>(operation: () => Promise<T>): Promise<Result<T>> {
  try { return success(await operation()); }
  catch (error) {
    if (error instanceof FileError) return failure(error.code, error.message);
    const code = error instanceof Error && 'code' in error ? error.code : null;
    if (code === 'ENOENT' || code === 'ENOTDIR') return failure('NOT_FOUND', 'File or directory not found.');
    if (code === 'EACCES' || code === 'EPERM' || code === 'ELOOP') return failure('ACCESS_DENIED', 'File access was denied.');
    return failure('STORAGE_FAILED', 'File operation failed.', true);
  }
}
export class FileService implements FileApi {
  readonly transfers: FileTransfers;
  constructor(private readonly context: FileSessionContext) { this.transfers = new FileTransfers(context); }
  initialize(userData: string): Promise<void> { return this.transfers.initialize(userData); }
  dispose(): Promise<void> { return this.transfers.dispose(); }
  list(input: FileLocation): Promise<Result<FileList>> {
    return result(async () => {
      const target = await resolveFileTarget(this.context, input), canonical = await realpath(target.native);
      const entries: FileList['entries'] = [];
      let truncated = false, bytes = Buffer.byteLength(canonical) * 2 + 256;
      const directory = await opendir(canonical);
      for await (const entry of directory) {
        const item = { name: entry.name, path: target.display(path.join(canonical, entry.name)), kind: entry.isDirectory() ? 'directory' as const : entry.isFile() ? 'file' as const : entry.isSymbolicLink() ? 'symlink' as const : 'other' as const };
        const size = Buffer.byteLength(JSON.stringify(item)) + 1;
        if (entries.length >= FILE_LIST_LIMIT || bytes + size > FILE_LIST_BYTES - 64) { truncated = true; break; }
        if (/[\x00-\x1f\x7f]/.test(entry.name)) continue;
        entries.push(item); bytes += size;
      }
      entries.sort((a, b) => Number(b.kind === 'directory') - Number(a.kind === 'directory') || a.name.localeCompare(b.name));
      const parent = path.dirname(canonical);
      return { path: target.display(canonical), parentPath: parent === canonical ? null : target.display(parent), entries, truncated };
    });
  }
  read(input: FileLocation): Promise<Result<FileRead>> {
    return result(async () => {
      const target = await resolveFileTarget(this.context, input), canonical = await realpath(target.native), bytes = await boundedRead(canonical);
      const image = imageMimeType(bytes), metadata = { path: target.display(canonical), size: bytes.length, revision: revision(bytes), mimeType: image ?? mimeType(canonical) };
      if (image) return { ...metadata, kind: 'image', mimeType: image, dataUrl: dataUrl(image, bytes) };
      try { return { ...metadata, kind: 'text', text: decodeText(bytes) }; }
      catch (error) { if (!(error instanceof FileError)) throw error; return { ...metadata, kind: 'binary', mimeType: 'application/octet-stream' }; }
    });
  }
  write(input: FileWriteInput, canCommit: () => boolean = () => true): Promise<Result<FileWriteResult>> {
    return result(async () => {
      if (!fileWriteSchema.safeParse(input).success) throw new FileError('VALIDATION', 'Invalid file save request.');
      return this.save(input, Buffer.from(input.text, 'utf8'), previous => { decodeText(previous); }, canCommit);
    });
  }
  writeImage(input: FileImageWriteInput, canCommit: () => boolean = () => true): Promise<Result<FileWriteResult>> {
    return result(async () => {
      if (!fileImageWriteSchema.safeParse(input).success) throw new FileError('VALIDATION', 'Invalid image save request.');
      const bytes = decodeImage(input.dataUrl, input.path);
      return this.save(input, bytes, (previous, canonical) => {
        if (imageMimeType(previous) !== mimeType(canonical)) throw new FileError('UNSUPPORTED', 'The existing file is not a supported image with a matching extension.');
        decodeImage(input.dataUrl, canonical);
      }, canCommit);
    });
  }
  private async save(input: FileLocation & { revision: string }, bytes: Buffer, validatePrevious: (bytes: Buffer, canonical: string) => void, canCommit: () => boolean): Promise<FileWriteResult> {
    const target = await resolveFileTarget(this.context, { sessionId: input.sessionId, tabId: input.tabId, path: input.path });
    const canonical = await realpath(target.native);
    const key = process.platform === 'win32' ? canonical.toLowerCase() : canonical;
    const pending = (writes.get(key) ?? Promise.resolve()).catch(() => {}).then(async () => {
      const info = await stat(canonical), previous = await boundedRead(canonical);
      validatePrevious(previous, canonical);
      if (revision(previous) !== input.revision) throw new FileError('FILE_CONFLICT', 'File changed on disk. Reload it before saving.');
      const stage = path.join(path.dirname(canonical), `.shellfox-save-${randomUUID()}`);
      try {
        const handle = await open(stage, 'wx', info.mode & 0o777);
        try { await handle.writeFile(bytes); if (process.platform !== 'win32') await handle.chmod(info.mode & 0o777); await handle.sync(); } finally { await handle.close(); }
        // Recheck immediately before replacement, including symlink retargeting and external saves.
        if (await realpath(target.native) !== canonical || !sameFile(info, await stat(canonical)) || revision(await boundedRead(canonical)) !== input.revision) throw new FileError('FILE_CONFLICT', 'File changed on disk. Reload it before saving.');
        if (!canCommit()) throw new FileError('ACCESS_DENIED', 'File save authorization expired.');
        await rename(stage, canonical);
      } finally { await unlink(stage).catch(() => {}); }
      return { path: target.display(canonical), size: bytes.length, revision: revision(bytes) };
    });
    writes.set(key, pending);
    try { return await pending; } finally { if (writes.get(key) === pending) writes.delete(key); }
  }
  readAsset(input: FileAssetInput): Promise<Result<FileAsset>> {
    return result(async () => {
      if (!fileAssetRequestSchema.safeParse(input).success) throw new FileError('VALIDATION', 'Invalid preview asset request.');
      const { assetPath, ...location } = input;
      if (/^(?:[/\\]|[a-z]:)|[?#]/i.test(assetPath) || (process.platform === 'win32' && assetPath.includes(':'))) throw new FileError('ACCESS_DENIED', 'Preview assets must use relative paths without a query or fragment.');
      const target = await resolveFileTarget(this.context, location), html = await realpath(target.native);
      if (!/\.(?:html?|xhtml|md|mdx|markdown|mdown|mkd)$/i.test(html) || !(await stat(html)).isFile()) throw new FileError('VALIDATION', 'Preview assets require an existing HTML or Markdown file.');
      const candidate = await realpath(path.resolve(path.dirname(html), assetPath));
      const relative = path.relative(path.dirname(html), candidate);
      if (relative === '..' || relative.startsWith('..' + path.sep) || path.isAbsolute(relative)) throw new FileError('ACCESS_DENIED', 'Preview asset is outside the HTML directory.');
      const type = assetMimeType(candidate), bytes = await boundedRead(candidate);
      return { mimeType: type, dataUrl: dataUrl(type, bytes), size: bytes.length };
    });
  }
}
