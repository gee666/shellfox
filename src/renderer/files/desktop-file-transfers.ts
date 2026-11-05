import type { FileApi, FileLocation } from '../../shared/file-contracts';
import type { DesktopFilesApi } from '../../shared/desktop-file-contracts';
import type { Result } from '../../shared/contracts';
import type { FileTransferApi } from '../../shared/file-transfer-contracts';
import { FILE_CHUNK_BYTES } from '../../shared/file-transfer-schemas';
import { decodeFileChunk } from '../../shared/file-api';

export function hasFileTransfers(files: FileApi | undefined): files is FileApi & FileTransferApi {
  return !!files?.beginRead && !!files.readChunk && !!files.beginWrite && !!files.writeChunk && !!files.finishWrite && !!files.cancelTransfer;
}
function unwrap<T>(result: Result<T>): T { if (!result.ok) throw new Error(result.error.message); return result.value; }
function active(signal: AbortSignal): void { if (signal.aborted) throw new Error('File transfer cancelled.'); }
type Progress = (offset: number, total: number, name: string) => void;
const fileName = (path: string) => path.split(/[\\/]/).at(-1) || 'download';

export async function uploadDesktopFile(files: FileTransferApi, local: DesktopFilesApi, location: FileLocation, signal: AbortSignal, progress: Progress): Promise<string | null> {
  let source: string | undefined, destination: string | undefined;
  const cleanup = async () => {
    await Promise.allSettled([
      ...(source ? [local.releaseUpload({ handle: source })] : []),
      ...(destination ? [files.cancelTransfer({ transferId: destination })] : []),
    ]);
  };
  const cancel = () => { void cleanup(); };
  signal.addEventListener('abort', cancel);
  try {
    active(signal);
    const selected = unwrap(await local.pickUpload());
    if (!selected) return null;
    source = selected.handle; active(signal);
    progress(0, selected.size, selected.name);
    const path = location.path + (/[\\/]$/.test(location.path) ? '' : '/') + selected.name;
    const begin = unwrap(await files.beginWrite({ ...location, path, size: selected.size, mode: 'create', content: 'file' }));
    destination = begin.transferId; active(signal);
    if (begin.chunkBytes !== FILE_CHUNK_BYTES || selected.chunkBytes !== FILE_CHUNK_BYTES) throw new Error('Unsupported file chunk size.');
    let offset = 0;
    while (offset < selected.size) {
      active(signal);
      const chunk = unwrap(await local.readUpload({ handle: source, offset }));
      active(signal);
      const bytes = decodeFileChunk(chunk.data);
      if (!bytes.length || bytes.length > FILE_CHUNK_BYTES || chunk.offset !== offset + bytes.length || chunk.offset > selected.size || chunk.eof !== (chunk.offset === selected.size)) throw new Error('Invalid local upload chunk.');
      const written = unwrap(await files.writeChunk({ transferId: destination, offset, data: chunk.data }));
      if (written.offset !== chunk.offset) throw new Error('The host returned an invalid upload offset.');
      offset = written.offset; progress(offset, selected.size, selected.name);
    }
    active(signal);
    unwrap(await files.finishWrite({ transferId: destination }));
    return selected.name;
  } finally { signal.removeEventListener('abort', cancel); await cleanup(); }
}

export async function downloadDesktopFile(files: FileTransferApi, local: DesktopFilesApi, location: FileLocation, signal: AbortSignal, progress: Progress): Promise<string | null> {
  let source: string | undefined, destination: string | undefined;
  const cleanup = async () => {
    await Promise.allSettled([
      ...(source ? [files.cancelTransfer({ transferId: source })] : []),
      ...(destination ? [local.cancelDownload({ handle: destination })] : []),
    ]);
  };
  const cancel = () => { void cleanup(); };
  signal.addEventListener('abort', cancel);
  try {
    active(signal);
    // Transfer metadata is independent of preview limits and failures.
    const selected = unwrap(await files.beginRead(location));
    source = selected.transferId; active(signal);
    const name = fileName(selected.path);
    progress(0, selected.size, name);
    // Do not hold a remote lease while a user keeps the native save dialog open.
    unwrap(await files.cancelTransfer({ transferId: source })); source = undefined;
    active(signal);
    const chosen = unwrap(await local.beginDownload({ name, size: selected.size }));
    if (!chosen) return null;
    destination = chosen.handle; active(signal);
    const stream = unwrap(await files.beginRead({ ...location, path: selected.path }));
    source = stream.transferId; active(signal);
    if (stream.size !== selected.size || stream.revision !== selected.revision) throw new Error('The host file changed while choosing the download destination.');
    if (stream.chunkBytes !== FILE_CHUNK_BYTES) throw new Error('Unsupported file chunk size.');
    let offset = 0;
    do {
      active(signal);
      const chunk = unwrap(await files.readChunk({ transferId: source, offset }));
      active(signal);
      const bytes = decodeFileChunk(chunk.data);
      if (bytes.length > FILE_CHUNK_BYTES || chunk.offset !== offset + bytes.length || chunk.offset > selected.size || chunk.eof !== (chunk.offset === selected.size) || !bytes.length && !chunk.eof) throw new Error('Invalid host download chunk.');
      if (bytes.length) {
        const written = unwrap(await local.writeDownload({ handle: destination, offset, data: chunk.data }));
        if (written.offset !== chunk.offset) throw new Error('Invalid local download offset.');
      }
      offset = chunk.offset; progress(offset, selected.size, name);
      if (chunk.eof) break;
    } while (offset < selected.size);
    active(signal);
    unwrap(await local.finishDownload({ handle: destination }));
    return name;
  } finally { signal.removeEventListener('abort', cancel); await cleanup(); }
}
