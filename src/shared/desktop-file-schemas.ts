import { z } from 'zod';
import { resultSchema } from './schemas';
import { FILE_CHUNK_BYTES, fileTransferSizeSchema, fileTransferRequestSchemas } from './file-transfer-schemas';

export const DESKTOP_FILES_CHANNEL = 'desktop-files:request';
export const localFileNameSchema = z.string().min(1).max(255).refine(name => name !== '.' && name !== '..' && !/[\\/\x00-\x1f\x7f]/.test(name));
const identity = z.object({ handle: z.string().uuid() }).strict();
export const desktopFileRequestSchemas = {
  pickUpload: z.object({}).strict(),
  readUpload: identity.extend({ offset: fileTransferSizeSchema }),
  releaseUpload: identity,
  beginDownload: z.object({ name: localFileNameSchema, size: fileTransferSizeSchema }).strict(),
  writeDownload: identity.extend({ offset: fileTransferSizeSchema, data: fileTransferRequestSchemas.writeFileChunk.shape.data }),
  finishDownload: identity,
  cancelDownload: identity,
};
export const desktopFileResponseSchemas = {
  pickUpload: resultSchema(z.object({ handle: z.string().uuid(), name: localFileNameSchema, size: fileTransferSizeSchema, chunkBytes: z.literal(FILE_CHUNK_BYTES) }).strict().nullable()),
  readUpload: resultSchema(z.object({ offset: fileTransferSizeSchema, data: fileTransferRequestSchemas.writeFileChunk.shape.data.or(z.literal('')), eof: z.boolean() }).strict()),
  releaseUpload: resultSchema(z.object({ released: z.literal(true) }).strict()),
  beginDownload: resultSchema(z.object({ handle: z.string().uuid() }).strict().nullable()),
  writeDownload: resultSchema(z.object({ offset: fileTransferSizeSchema }).strict()),
  finishDownload: resultSchema(z.object({ saved: z.literal(true) }).strict()),
  cancelDownload: resultSchema(z.object({ cancelled: z.literal(true) }).strict()),
};
export type DesktopFileMethod = keyof typeof desktopFileRequestSchemas;
export const desktopFileEnvelopeSchema = z.object({ version: z.literal(1), method: z.enum(['pickUpload', 'readUpload', 'releaseUpload', 'beginDownload', 'writeDownload', 'finishDownload', 'cancelDownload']), payload: z.unknown() }).strict();
