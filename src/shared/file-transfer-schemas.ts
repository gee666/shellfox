import { z } from 'zod';
import { canonicalBase64 } from './file-base64';
import { fileLocationSchema, filePathSchema, fileRevisionSchema } from './file-schemas';

export const FILE_CHUNK_BYTES = 192 * 1024;
export const FILE_TRANSFER_MAX_BYTES = 1024 * 1024 * 1024;
export const FILE_TRANSFER_DEFAULT_BYTES = 256 * 1024 * 1024;
export const FILE_TRANSFER_TTL_MS = 5 * 60_000;
export const fileTransferSizeSchema = z.number().int().nonnegative().max(FILE_TRANSFER_MAX_BYTES);
export const fileTransferIdentitySchema = z.object({ transferId: z.string().uuid() }).strict();
const data = z.string().refine(value => value.length <= FILE_CHUNK_BYTES / 3 * 4 && canonicalBase64(value));
export const fileBeginReadSchema = fileLocationSchema.safeExtend({ assetPath: filePathSchema.optional() });
export const fileBeginWriteSchema = fileLocationSchema.safeExtend({ size: fileTransferSizeSchema, mode: z.enum(['create', 'replace']), revision: fileRevisionSchema.optional(), content: z.enum(['file', 'text', 'image']), mimeType: z.enum(['image/png', 'image/jpeg', 'image/webp']).optional() }).refine(value => (value.mode === 'replace') === !!value.revision && (value.content === 'image') === !!value.mimeType);
export const fileTransferRequestSchemas = {
  beginFileRead: fileBeginReadSchema,
  readFileChunk: fileTransferIdentitySchema.extend({ offset: fileTransferSizeSchema }),
  beginFileWrite: fileBeginWriteSchema,
  writeFileChunk: fileTransferIdentitySchema.extend({ offset: fileTransferSizeSchema, data }),
  finishFileWrite: fileTransferIdentitySchema,
  cancelFileTransfer: fileTransferIdentitySchema,
};
export const fileTransferValueSchemas = {
  beginFileRead: z.object({ transferId: z.string().uuid(), path: filePathSchema, size: fileTransferSizeSchema, revision: fileRevisionSchema, kind: z.enum(['text', 'image', 'binary']), mimeType: z.string().min(1).max(100), chunkBytes: z.literal(FILE_CHUNK_BYTES) }).strict(),
  readFileChunk: z.object({ offset: fileTransferSizeSchema, data: data.or(z.literal('')), eof: z.boolean() }).strict(),
  beginFileWrite: z.object({ transferId: z.string().uuid(), chunkBytes: z.literal(FILE_CHUNK_BYTES) }).strict(),
  writeFileChunk: z.object({ offset: fileTransferSizeSchema }).strict(),
  finishFileWrite: z.object({ path: filePathSchema, size: fileTransferSizeSchema, revision: fileRevisionSchema }).strict(),
  cancelFileTransfer: z.object({ cancelled: z.literal(true) }).strict(),
};
export type FileTransferMethod = keyof typeof fileTransferRequestSchemas;
export const isFileTransferMethod = (method: string): method is FileTransferMethod => Object.hasOwn(fileTransferRequestSchemas, method);
