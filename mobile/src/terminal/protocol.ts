import { z } from 'zod';
export const utf8Size = (text: string): number => new TextEncoder().encode(text).byteLength;

const token = z.string().min(1).max(100);
const dimension = z.number().int().min(2).max(500);
const sequence = z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER);
export const terminalFontSize = (size: number): number => Number.isFinite(size) ? Math.max(8, Math.min(32, size)) : 14;
const input = z.string().min(1).max(65536).refine(s => !s.includes('\0') && utf8Size(s) <= 65536);
const identity = { viewId: token, epoch: token };
const themeColor = z.string().regex(/^#[0-9a-fA-F]{6}$/);
export const terminalThemeSchema = z.object({ accentColor: themeColor, backgroundColor: themeColor }).strict();
export type TerminalThemeInputs = z.infer<typeof terminalThemeSchema>;
export const terminalModifiersSchema = z.object({
  ctrl: z.boolean(), alt: z.boolean(), shift: z.boolean(), meta: z.boolean(), capsLock: z.boolean(),
}).strict();
export type TerminalModifiers = z.infer<typeof terminalModifiersSchema>;
export const emptyModifiers: TerminalModifiers = { ctrl: false, alt: false, shift: false, meta: false, capsLock: false };
export const terminalKeySchema = z.union([
  z.enum(['Escape', 'Tab', 'Enter', 'Backspace', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight',
    'Home', 'End', 'PageUp', 'PageDown', 'Insert', 'Delete',
    'F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8', 'F9', 'F10', 'F11', 'F12']),
  z.string().length(1).regex(/^[\x20-\x7e]$/),
]);
export type TerminalKey = z.infer<typeof terminalKeySchema>;
export const terminalKeyEventSchema = z.object({ key: terminalKeySchema, modifiers: terminalModifiersSchema }).strict();
export type TerminalKeyEvent = z.infer<typeof terminalKeyEventSchema>;
export const pageMessageSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('ready'), viewId: token }).strict(),
  z.object({ type: z.literal('input'), ...identity, data: input }).strict(),
  z.object({ type: z.literal('resize'), ...identity, cols: dimension, rows: dimension }).strict(),
  z.object({ type: z.literal('rendered'), ...identity, ticket: sequence }).strict(),
  z.object({ type: z.literal('focused'), ...identity }).strict(),
  z.object({ type: z.literal('selection'), ...identity, text: z.string().max(65536) }).strict(),
  z.object({ type: z.literal('error'), ...identity, message: z.string().max(500) }).strict(),
]);
export type PageMessage = z.infer<typeof pageMessageSchema>;
export const hostMessageSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('clear'), ...identity }).strict(),
  z.object({ type: z.literal('reset'), ...identity, cols: dimension, rows: dimension, fontSize: z.number().min(8).max(32) }).strict(),
  z.object({ type: z.literal('output'), ...identity, ticket: sequence, data: z.string().max(262144), historical: z.boolean(), fit: z.boolean() }).strict(),
  z.object({ type: z.literal('font'), ...identity, fontSize: z.number().min(8).max(32) }).strict(),
  z.object({ type: z.literal('theme'), ...identity, ...terminalThemeSchema.shape }).strict(),
  z.object({ type: z.literal('paste'), ...identity, data: input }).strict(),
  z.object({ type: z.literal('key'), ...identity, event: terminalKeyEventSchema }).strict(),
  z.object({ type: z.literal('modifiers'), ...identity, modifiers: terminalModifiersSchema }).strict(),
  z.object({ type: z.literal('focus'), ...identity, focused: z.boolean() }).strict(),
  z.object({ type: z.literal('select'), ...identity, enabled: z.boolean() }).strict(),
]);
export type HostMessage = z.infer<typeof hostMessageSchema>;

export function parsePageMessage(json: string): PageMessage | null {
  if (json.length > 400000) return null;
  try { const parsed = pageMessageSchema.safeParse(JSON.parse(json)); return parsed.success ? parsed.data : null; }
  catch { return null; }
}

// The initial inline document is the only permitted top-level navigation.
export function allowTerminalNavigation(url: string): boolean { return url === 'about:blank'; }
