import type { z } from 'zod';
import type { FileTransferApi } from './file-transfer-contracts';
import type { Result } from './contracts';
import type { fileLocationSchema, fileWriteSchema, fileImageWriteSchema, fileAssetRequestSchema, fileListSchema, fileReadSchema, fileWriteResultSchema, fileAssetSchema } from './file-schemas';

export type FileLocation = z.infer<typeof fileLocationSchema>;
export type FileImageWriteInput = z.infer<typeof fileImageWriteSchema>;
export type FileWriteInput = z.infer<typeof fileWriteSchema>;
export type FileAssetInput = z.infer<typeof fileAssetRequestSchema>;
export type FileList = z.infer<typeof fileListSchema>;
export type FileRead = z.infer<typeof fileReadSchema>;
export type FileWriteResult = z.infer<typeof fileWriteResultSchema>;
export type FileAsset = z.infer<typeof fileAssetSchema>;
export interface FileApi extends Partial<FileTransferApi> {
  /** Relative paths use this tab's reported working directory, or its launch directory until reported. */
  list(input: FileLocation): Promise<Result<FileList>>;
  read(input: FileLocation): Promise<Result<FileRead>>;
  /** Only existing UTF-8 text files can be saved. FILE_CONFLICT means reload before saving. */
  write(input: FileWriteInput): Promise<Result<FileWriteResult>>;
  /** Existing PNG/JPEG/WebP only. Production clients save through bounded chunks. */
  writeImage(input: FileImageWriteInput): Promise<Result<FileWriteResult>>;
  /** path identifies the HTML file. assetPath must stay under its canonical parent directory. */
  readAsset(input: FileAssetInput): Promise<Result<FileAsset>>;
}
