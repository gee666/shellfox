import { z } from 'zod';
export const utf8Size = (text: string): number => new TextEncoder().encode(text).byteLength;

const token = z.string().min(1).max(100);
const dimension = z.number().int().min(2).max(500);
const sequence = z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER);
export const terminalFontSize = (size: number): number => Number.isFinite(size) ? Math.max(8, Math.min(32, size)) : 14;
const input = z.string().min(1).max(65536).refine(s => !s.includes('\0') && utf8Size(s) <= 65536);
const identity = { viewId: token, epoch: token };
export const pageMessageSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('ready'), viewId: token }).strict(),
  z.object({ type: z.literal('input'), ...identity, data: input }).strict(),
  z.object({ type: z.literal('resize'), ...identity, cols: dimension, rows: dimension }).strict(),
  z.object({ type: z.literal('rendered'), ...identity, ticket: sequence }).strict(),
  z.object({ type: z.literal('selection'), ...identity, text: z.string().max(65536) }).strict(),
  z.object({ type: z.literal('error'), ...identity, message: z.string().max(500) }).strict(),
]);
export type PageMessage = z.infer<typeof pageMessageSchema>;
export const hostMessageSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('reset'), ...identity, cols: dimension, rows: dimension, fontSize: z.number().min(8).max(32) }).strict(),
  z.object({ type: z.literal('output'), ...identity, ticket: sequence, data: z.string().max(262144), historical: z.boolean(), fit: z.boolean() }).strict(),
  z.object({ type: z.literal('font'), ...identity, fontSize: z.number().min(8).max(32) }).strict(),
  z.object({ type: z.literal('paste'), ...identity, data: input }).strict(),
  z.object({ type: z.literal('focus'), ...identity, focused: z.boolean() }).strict(),
  z.object({ type: z.literal('select'), ...identity, enabled: z.boolean() }).strict(),
]);
export type HostMessage = z.infer<typeof hostMessageSchema>;

export function parsePageMessage(json: string): PageMessage | null {
  if (json.length > 400000) return null;
  try { const parsed = pageMessageSchema.safeParse(JSON.parse(json)); return parsed.success ? parsed.data : null; }
  catch { return null; }
}

export function controlInput(data: string, control: boolean): string {
  if (!control || data.length !== 1) return data;
  if (data === '?') return '\x7f';
  const code = data.toUpperCase().charCodeAt(0);
  return code >= 64 && code <= 95 ? String.fromCharCode(code & 31) : data;
}

// The initial inline document is the only permitted top-level navigation.
export function allowTerminalNavigation(url: string): boolean { return url === 'about:blank'; }
