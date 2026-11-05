import { ipcRenderer } from 'electron';
import type { DesktopFilesApi, DesktopFileResult } from '../shared/desktop-file-contracts';
import { DESKTOP_FILES_CHANNEL, desktopFileRequestSchemas, desktopFileResponseSchemas, type DesktopFileMethod } from '../shared/desktop-file-schemas';
import { failure } from '../shared/contracts';

function request<M extends DesktopFileMethod>(method: M, payload: unknown): Promise<DesktopFileResult<M>>;
async function request(method: DesktopFileMethod, payload: unknown): Promise<DesktopFileResult<DesktopFileMethod>> {
  if (!desktopFileRequestSchemas[method].safeParse(payload).success) return failure('VALIDATION', 'Invalid local file request.');
  try {
    const result = desktopFileResponseSchemas[method].safeParse(await ipcRenderer.invoke(DESKTOP_FILES_CHANNEL, { version: 1, method, payload }));
    return result.success ? result.data : failure('INTERNAL', 'Invalid local file response.');
  } catch { return failure('STORAGE_FAILED', 'Local file operation failed.', true); }
}
export const desktopFilesApi = Object.freeze<DesktopFilesApi>({
  pickUpload: () => request('pickUpload', {}),
  readUpload: input => request('readUpload', input),
  releaseUpload: input => request('releaseUpload', input),
  beginDownload: input => request('beginDownload', input),
  writeDownload: input => request('writeDownload', input),
  finishDownload: input => request('finishDownload', input),
  cancelDownload: input => request('cancelDownload', input),
});
