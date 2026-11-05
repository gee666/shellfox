import { NativeModules } from 'react-native';
import { z } from 'zod';
import { withAttachmentPicker } from './attachment-picker-lifecycle';
import { FILE_CHUNK_BYTES, fileTransferSizeSchema } from '../../src/shared/file-transfer-schemas';

const handle = z.string().uuid();
const saved = z.object({ fileName: z.string().min(1), location: z.enum(['Downloads', 'Files']) }).strict();
export type SavedDownload = z.infer<typeof saved>;
export interface Downloads {
  begin(token: string, fileName: string, size: number, mimeType: string): Promise<{ handle: string } | null>;
  write(handle: string, offset: number, data: string): Promise<{ offset: number }>;
  finish(handle: string): Promise<SavedDownload | null>;
  cancel(handle: string): Promise<void>;
}
const nativeSchema = z.object({
  begin: z.function({ input: [handle, z.string().min(1).max(255).refine(value => value !== '.' && value !== '..' && !/[\x00-\x1f\x7f/\\]/.test(value)), fileTransferSizeSchema, z.string().regex(/^[A-Za-z0-9.+-]+\/[A-Za-z0-9.+-]+$/)], output: z.promise(z.object({ handle }).strict().nullable()) }),
  write: z.function({ input: [handle, fileTransferSizeSchema, z.string().min(4).max(FILE_CHUNK_BYTES * 4 / 3).regex(/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/)], output: z.promise(z.object({ offset: fileTransferSizeSchema }).strict()) }),
  finish: z.function({ input: [handle], output: z.promise(saved.nullable()) }),
  cancel: z.function({ input: [handle], output: z.promise(z.null()) }),
});
function nativeDownloads() {
  const modules: Record<string, unknown> = NativeModules;
  const parsed = nativeSchema.safeParse(modules.ShellfoxDownloads);
  if (!parsed.success) throw new Error('Downloads require a newer Shellfox mobile app.');
  return parsed.data;
}
export const downloads: Downloads = {
  begin: (...input) => withAttachmentPicker(() => nativeDownloads().begin(...input)),
  write: (...input) => nativeDownloads().write(...input),
  finish: handle => withAttachmentPicker(() => nativeDownloads().finish(handle)),
  cancel: async handle => { await nativeDownloads().cancel(handle); },
};
