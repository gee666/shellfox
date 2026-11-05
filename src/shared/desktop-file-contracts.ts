import type { z } from 'zod';
import type { desktopFileRequestSchemas, desktopFileResponseSchemas, DesktopFileMethod } from './desktop-file-schemas';

export type DesktopFileInput<M extends DesktopFileMethod> = z.infer<(typeof desktopFileRequestSchemas)[M]>;
export type DesktopFileResult<M extends DesktopFileMethod> = z.infer<(typeof desktopFileResponseSchemas)[M]>;
/** Local desktop capabilities. Paths never cross the preload bridge or a remote connection. */
export interface DesktopFilesApi {
  pickUpload(): Promise<DesktopFileResult<'pickUpload'>>;
  readUpload(input: DesktopFileInput<'readUpload'>): Promise<DesktopFileResult<'readUpload'>>;
  releaseUpload(input: DesktopFileInput<'releaseUpload'>): Promise<DesktopFileResult<'releaseUpload'>>;
  beginDownload(input: DesktopFileInput<'beginDownload'>): Promise<DesktopFileResult<'beginDownload'>>;
  writeDownload(input: DesktopFileInput<'writeDownload'>): Promise<DesktopFileResult<'writeDownload'>>;
  finishDownload(input: DesktopFileInput<'finishDownload'>): Promise<DesktopFileResult<'finishDownload'>>;
  cancelDownload(input: DesktopFileInput<'cancelDownload'>): Promise<DesktopFileResult<'cancelDownload'>>;
}
