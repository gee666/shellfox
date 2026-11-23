import { z } from 'zod';

/** Ghostty's key event: the physical key (public GhosttyKey number), what the
 * layout typed, and the modifier bits. Ghostty turns it into terminal bytes. */
export type KeyEvent = Readonly<{
  physical: number; action: 'press' | 'repeat' | 'release';
  modifiers: number; consumedModifiers: number;
  text: string; unshiftedCodepoint: number | null; composing: boolean;
}>;

// Native keyboards and the key bar send this JSON to Rust. Nothing here
// builds terminal escape sequences.
export type TerminalInput =
  | Readonly<{ type: 'editor'; committed: string }>
  | Readonly<{ type: 'editorReset' }>
  | Readonly<{ type: 'paste'; text: string }>
  | Readonly<{ type: 'key'; event: KeyEvent }>
  | Readonly<{
      type: 'mouse'; action: 'press' | 'release' | 'motion';
      button?: 'left' | 'middle' | 'right' | 'wheelUp' | 'wheelDown'
        | 'wheelLeft' | 'wheelRight' | 'back' | 'forward' | null;
      modifiers?: number; x: number; y: number;
      cellWidth: number; cellHeight: number; anyButtonPressed: boolean;
    }>;

const keyText = z.string().refine((value) => !/[\x00-\x1f\x7f\uf700-\uf8ff]/u.test(value));
const keyModifiers = z.number().int().min(0).max(0x3ff);
const keyEvent = z.object({
  physical: z.number().int().min(0).max(175),
  action: z.enum(['press', 'repeat', 'release']),
  modifiers: keyModifiers, consumedModifiers: keyModifiers, text: keyText,
  unshiftedCodepoint: z.number().int().min(1).max(0x10ffff)
    .refine((value) => value < 0xd800 || value > 0xdfff).nullable(),
  composing: z.boolean(),
}).strict();
export const terminalInputSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('editor'), committed: z.string().refine((value) => !/[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]/.test(value)) }).strict(),
  z.object({ type: z.literal('editorReset') }).strict(),
  z.object({ type: z.literal('paste'), text: z.string() }).strict(),
  z.object({ type: z.literal('key'), event: keyEvent }).strict(),
  z.object({
    type: z.literal('mouse'), action: z.enum(['press', 'release', 'motion']),
    button: z.enum(['left', 'middle', 'right', 'wheelUp', 'wheelDown', 'wheelLeft',
      'wheelRight', 'back', 'forward']).nullable().optional(),
    modifiers: z.number().int().min(0).max(15).optional(),
    x: z.number().min(-1e9).max(1e9), y: z.number().min(-1e9).max(1e9),
    cellWidth: z.number().int().min(1).max(1e9), cellHeight: z.number().int().min(1).max(1e9),
    anyButtonPressed: z.boolean(),
  }).strict(),
]);

export function encodeTerminalInput(event: TerminalInput): string {
  const json = JSON.stringify(terminalInputSchema.parse(event));
  if (new TextEncoder().encode(json).byteLength > 65536) throw new Error('Terminal input exceeds limit.');
  return json;
}
