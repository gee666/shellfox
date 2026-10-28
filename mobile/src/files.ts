import { NativeModules } from 'react-native';
import { z } from 'zod';

export const MAX_PICKED_FILE_BYTES = 64 * 1024 * 1024;
export const MAX_FILE_CHUNK_BYTES = 192 * 1024;
const handleSchema = z.string().uuid();
const offsetSchema = z.number().int().min(0).max(MAX_PICKED_FILE_BYTES);
const lengthSchema = z.number().int().min(1).max(MAX_FILE_CHUNK_BYTES);
export const pickedFileSchema = z.object({
  handle: handleSchema,
  fileName: z.string().min(1).max(1024).refine(name => !/[\x00-\x1f\x7f]/.test(name)),
  size: offsetSchema,
}).strict();
const chunkSchema = z.object({
  data: z.string().max(MAX_FILE_CHUNK_BYTES * 4 / 3).regex(/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/),
  bytes: lengthSchema,
}).strict();
export type PickedFile = z.infer<typeof pickedFileSchema>;
export interface PickedFiles {
  pick(token: string): Promise<PickedFile | null>;
  read(handle: string, offset: number, length: number): Promise<{ data: string; bytes: number }>;
  release(handle: string): Promise<void>;
  cancelPick(token: string): Promise<void>;
}
const nativeSchema = z.object({
  pick: z.function({ input: [handleSchema], output: z.promise(pickedFileSchema.nullable()) }),
  read: z.function({ input: [handleSchema, offsetSchema, lengthSchema], output: z.promise(chunkSchema) }),
  release: z.function({ input: [handleSchema], output: z.promise(z.null()) }),
  cancelPick: z.function({ input: [handleSchema], output: z.promise(z.null()) }),
});
function nativeFiles() {
  const modules: Record<string, unknown> = NativeModules;
  const parsed = nativeSchema.safeParse(modules.ShellfoxFiles);
  if (!parsed.success) throw new Error('File attachments need a newer Shellfox mobile app.');
  return parsed.data;
}
export const pickedFiles: PickedFiles = {
  pick: token => nativeFiles().pick(token),
  read: async (handle, offset, length) => {
    const chunk = await nativeFiles().read(handle, offset, length);
    const padding = chunk.data.endsWith('==') ? 2 : chunk.data.endsWith('=') ? 1 : 0;
    if (chunk.bytes !== chunk.data.length * 3 / 4 - padding || chunk.bytes !== length) {
      throw new Error('The selected file changed or could not be read completely.');
    }
    // Reject noncanonical padding bits without decoding the chunk into another JS buffer.
    const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
    if ((padding === 2 && (alphabet.indexOf(chunk.data.at(-3)!) & 15) !== 0)
      || (padding === 1 && (alphabet.indexOf(chunk.data.at(-2)!) & 3) !== 0)) throw new Error('Invalid file data.');
    return chunk;
  },
  release: async handle => { await nativeFiles().release(handle); },
  cancelPick: async token => { await nativeFiles().cancelPick(token); },
};
