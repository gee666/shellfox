import type { z } from 'zod';
import type { Result } from './contracts';
import type { fileTransferRequestSchemas, fileTransferValueSchemas } from './file-transfer-schemas';
export type FileTransferInput<M extends keyof typeof fileTransferRequestSchemas> = z.infer<(typeof fileTransferRequestSchemas)[M]>;
export type FileTransferValue<M extends keyof typeof fileTransferValueSchemas> = z.infer<(typeof fileTransferValueSchemas)[M]>;
export interface FileTransferApi {
  beginRead(input: FileTransferInput<'beginFileRead'>): Promise<Result<FileTransferValue<'beginFileRead'>>>;
  readChunk(input: FileTransferInput<'readFileChunk'>): Promise<Result<FileTransferValue<'readFileChunk'>>>;
  beginWrite(input: FileTransferInput<'beginFileWrite'>): Promise<Result<FileTransferValue<'beginFileWrite'>>>;
  writeChunk(input: FileTransferInput<'writeFileChunk'>): Promise<Result<FileTransferValue<'writeFileChunk'>>>;
  finishWrite(input: FileTransferInput<'finishFileWrite'>): Promise<Result<FileTransferValue<'finishFileWrite'>>>;
  cancelTransfer(input: FileTransferInput<'cancelFileTransfer'>): Promise<Result<FileTransferValue<'cancelFileTransfer'>>>;
}
