import { z } from 'zod';
import type { Result } from './contracts';
import type { FileRead, FileWriteResult } from './file-contracts';
import { FILE_IMAGE_BYTES, FILE_IMAGE_WRITE_BYTES } from './file-schemas';
import { validDataUrl } from './file-base64';

export const IMAGE_EDITOR_MAX_PIXELS = 16_000_000;
export const IMAGE_EDITOR_MAX_DIMENSION = 8192;
export const IMAGE_EDITOR_MAX_OPERATIONS = 256;
export const imageEditorMimeSchema = z.enum(['image/png', 'image/jpeg', 'image/webp']);
const coordinate = z.number().int().min(0).max(IMAGE_EDITOR_MAX_DIMENSION);
export const imageEditorOperationSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('rotate'), direction: z.enum(['left', 'right']) }).strict(),
  z.object({ type: z.literal('flip'), axis: z.enum(['horizontal', 'vertical']) }).strict(),
  z.object({ type: z.literal('crop'), x: coordinate, y: coordinate, width: coordinate.min(1), height: coordinate.min(1) }).strict(),
]);
export const imageEditorOperationsSchema = z.array(imageEditorOperationSchema).max(IMAGE_EDITOR_MAX_OPERATIONS);
const token = z.string().regex(/^[a-zA-Z0-9_-]{16,100}$/);
export const imageEditorDataUrlSchema = z.string().refine(value => validDataUrl(value, FILE_IMAGE_WRITE_BYTES, true));
export const imageEditorMessageSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('request-image'), token }).strict(),
  z.object({ type: z.literal('save-request'), token }).strict(),
  z.object({ type: z.literal('state'), token, dirty: z.boolean(), width: coordinate.min(1), height: coordinate.min(1), operations: imageEditorOperationsSchema }).strict(),
  z.object({ type: z.literal('export'), token, dataUrl: imageEditorDataUrlSchema }).strict(),
  z.object({ type: z.literal('error'), token, message: z.string().min(1).max(500) }).strict(),
]);
export type ImageEditorOperation = z.infer<typeof imageEditorOperationSchema>;
export type ImageEditorMessage = z.infer<typeof imageEditorMessageSchema>;
export type ImageEditorCommand = { type: 'export' | 'saved' | 'unlock'; token: string }
  | { type: 'load'; token: string; dataUrl: string };
export type EditableImage = Extract<FileRead, { kind: 'image' }>;
export interface FileImageEditorProps {
  image: EditableImage;
  /** Stable file API identity. Separates drafts belonging to different hosts. */
  draftScope?: object;
  onSave: (dataUrl: string) => Promise<Result<FileWriteResult>>;
  onSaved?: (result: FileWriteResult, dataUrl: string) => void;
  onDirtyChange?: (dirty: boolean) => void;
}

export function parseImageEditorMessage(value: unknown, expectedToken: string): ImageEditorMessage | null {
  // Native WebViews send strings. Reject oversized input before parsing.
  if (typeof value === 'string') {
    if (value.length > Math.ceil(FILE_IMAGE_WRITE_BYTES / 3) * 4 + 1024) return null;
    try { value = JSON.parse(value); } catch { return null; }
  }
  const result = imageEditorMessageSchema.safeParse(value);
  return result.success && result.data.token === expectedToken ? result.data : null;
}

export function validateEditorImage(image: EditableImage): void {
  const mime = imageEditorMimeSchema.safeParse(image.mimeType);
  if (!mime.success) throw new Error('Only PNG, JPEG and WebP images can be edited.');
  if (!image.dataUrl.startsWith(`data:${mime.data};base64,`) || !validDataUrl(image.dataUrl, FILE_IMAGE_BYTES, true)) {
    throw new Error('Invalid or oversized image.');
  }
}

export function allowImageEditorNavigation(url: string): boolean {
  return url === 'about:blank' || url === 'about:srcdoc';
}
