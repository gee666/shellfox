import { z } from 'zod';
import { validDataUrl } from './file-base64';

export const FILE_MAX_BYTES = 1024 * 1024 * 1024;
export const FILE_TEXT_BYTES = 8 * 1024 * 1024;
export const FILE_IMAGE_BYTES = 64 * 1024 * 1024;
export const FILE_IMAGE_WRITE_BYTES = FILE_IMAGE_BYTES;
// Legacy single-message endpoints remain small. Production clients use chunks.
export const FILE_INLINE_TEXT_BYTES = 128 * 1024;
export const FILE_INLINE_PREVIEW_BYTES = 2 * 1024 * 1024;
export const FILE_INLINE_IMAGE_WRITE_BYTES = 512 * 1024;
export const FILE_LIST_LIMIT = 500;
export const FILE_LIST_BYTES = 256 * 1024;
export const filePathSchema = z.string().min(1).max(32767).refine(value => !/[\x00-\x1f\x7f]/.test(value) && !value.includes('://') && !/^[a-z][a-z0-9+.-]+:/i.test(value));
export const fileLocationSchema = z.object({ sessionId: z.string().uuid().optional(), tabId: z.string().uuid().optional(), path: filePathSchema }).strict().refine(value => !value.tabId || !!value.sessionId, 'A tab requires its session identity.');
export const fileRevisionSchema = z.string().regex(/^[a-f0-9]{64}$/);
const text = z.string().max(FILE_TEXT_BYTES).refine(value => !value.includes('\0') && new TextEncoder().encode(value).byteLength <= FILE_TEXT_BYTES);
export const fileEditorWriteSchema = fileLocationSchema.safeExtend({ text, revision: fileRevisionSchema });
export const fileEditorImageWriteSchema = fileLocationSchema.safeExtend({ revision: fileRevisionSchema, dataUrl: z.string().refine(value => validDataUrl(value, FILE_IMAGE_WRITE_BYTES, true)) });
export const fileWriteSchema = fileLocationSchema.safeExtend({ text: text.refine(value => new TextEncoder().encode(value).byteLength <= FILE_INLINE_TEXT_BYTES), revision: fileRevisionSchema });
export const fileAssetRequestSchema = fileLocationSchema.safeExtend({ assetPath: filePathSchema });
export const fileImageWriteSchema = fileLocationSchema.safeExtend({ revision: fileRevisionSchema, dataUrl: z.string().refine(value => validDataUrl(value, FILE_INLINE_IMAGE_WRITE_BYTES, true)) });
export const fileEntrySchema = z.object({ name: filePathSchema, path: filePathSchema, kind: z.enum(['file', 'directory', 'symlink', 'other']) }).strict();
export const fileListSchema = z.object({ path: filePathSchema, parentPath: filePathSchema.nullable(), entries: z.array(fileEntrySchema).max(FILE_LIST_LIMIT), truncated: z.boolean() }).strict().refine(value => new TextEncoder().encode(JSON.stringify(value)).byteLength <= FILE_LIST_BYTES);
const metadata = { path: filePathSchema, mimeType: z.string().min(1).max(100), size: z.number().int().nonnegative().max(FILE_MAX_BYTES), revision: fileRevisionSchema };
const dataUrl = z.string().refine(value => validDataUrl(value, FILE_IMAGE_BYTES));
const rasterMime = z.enum(['image/png', 'image/jpeg', 'image/gif', 'image/webp', 'image/x-icon', 'image/avif']);
export const fileAssetMimeSchema = z.enum([...rasterMime.options, 'text/css', 'text/javascript', 'application/json', 'image/svg+xml', 'font/woff', 'font/woff2', 'font/ttf', 'font/otf']);
const matchingDataUrl = (value: { dataUrl: string; mimeType: string; size: number }) => {
  if (!value.dataUrl.startsWith(`data:${value.mimeType};base64,`)) return false;
  const encoded = value.dataUrl.slice(value.dataUrl.indexOf(',') + 1);
  return encoded.length / 4 * 3 - (encoded.endsWith('==') ? 2 : encoded.endsWith('=') ? 1 : 0) === value.size;
};
export const fileReadSchema = z.discriminatedUnion('kind', [
  z.object({ ...metadata, kind: z.literal('text'), text }).strict().refine(value => new TextEncoder().encode(value.text).byteLength === value.size),
  z.object({ ...metadata, kind: z.literal('image'), mimeType: rasterMime, dataUrl }).strict().refine(matchingDataUrl),
  z.object({ ...metadata, kind: z.literal('binary') }).strict(),
]);
export const fileWriteResultSchema = z.object({ path: filePathSchema, revision: fileRevisionSchema, size: z.number().int().nonnegative().max(FILE_IMAGE_BYTES) }).strict();
export const fileAssetSchema = z.object({ mimeType: fileAssetMimeSchema, dataUrl, size: z.number().int().nonnegative().max(FILE_IMAGE_BYTES) }).strict().refine(matchingDataUrl);
