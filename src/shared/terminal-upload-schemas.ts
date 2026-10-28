import { z } from 'zod';
import { failure } from './contracts';
import { controlEpochSchema, idSchema, pathSchema, resultSchema } from './schemas';

export const TERMINAL_UPLOAD_CHUNK_BYTES = 192 * 1024;
export const TERMINAL_UPLOAD_DEFAULT_MAX_BYTES = 64 * 1024 * 1024;
// The protocol ceiling bounds host configuration and integer arithmetic.
export const TERMINAL_UPLOAD_MAX_BYTES = 1024 * 1024 * 1024;
const size = z.number().int().min(0).max(TERMINAL_UPLOAD_MAX_BYTES);
const identity = { tabId: idSchema, generation: idSchema, controlEpoch: controlEpochSchema };
const upload = { ...identity, uploadId: idSchema };
const fileName = z.string().min(1).max(1024);
// Reject noncanonical encodings, including nonzero unused padding bits.
const data = z.string().min(4).max(TERMINAL_UPLOAD_CHUNK_BYTES / 3 * 4).regex(/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/)
  .refine(s => !s.endsWith('==') || 'AQgw'.includes(s[s.length - 3]!))
  .refine(s => !s.endsWith('=') || s.endsWith('==') || 'AEIMQUYcgkosw048'.includes(s[s.length - 2]!));
export const uploadRequestSchemas = {
  beginTerminalUpload: z.object({ ...identity, fileName, size }).strict(),
  writeTerminalUpload: z.object({ ...upload, offset: size, data }).strict(),
  finishTerminalUpload: z.object(upload).strict(),
  cancelTerminalUpload: z.object(upload).strict(),
};
export const uploadResponseSchemas = {
  beginTerminalUpload: resultSchema(z.object({ uploadId: idSchema, chunkBytes: z.literal(TERMINAL_UPLOAD_CHUNK_BYTES) }).strict()),
  writeTerminalUpload: resultSchema(z.object({ offset: size }).strict()),
  finishTerminalUpload: resultSchema(z.object({ path: pathSchema, fileName, size }).strict()),
  cancelTerminalUpload: resultSchema(z.object({ cancelled: z.literal(true) }).strict()),
};
export type TerminalUploadMethod = keyof typeof uploadRequestSchemas;
export type BeginTerminalUpload = z.infer<typeof uploadRequestSchemas.beginTerminalUpload>;
export type WriteTerminalUpload = z.infer<typeof uploadRequestSchemas.writeTerminalUpload>;
export type TerminalUploadIdentity = z.infer<typeof uploadRequestSchemas.finishTerminalUpload>;
export function isTerminalUploadMethod(method: string): method is TerminalUploadMethod {
  return Object.hasOwn(uploadRequestSchemas, method);
}
export function uploadResponseCompatibility(method: string, raw: unknown): unknown {
  if (!isTerminalUploadMethod(method)) return raw;
  const result = uploadResponseSchemas[method].safeParse(raw);
  if (result.success && !result.data.ok && result.data.error.code === 'AUTH_FAILED' && result.data.error.message === 'This operation is not available remotely.') {
    return failure('UNSUPPORTED', 'Upgrade the host to attach files to a terminal.');
  }
  return raw;
}
