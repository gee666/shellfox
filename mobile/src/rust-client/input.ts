import { z } from 'zod';

export type ModifierBits = number;
export type PhysicalKey =
  | Readonly<{ kind: 'unidentified' | 'enter' | 'tab' | 'backspace' | 'escape'
      | 'arrowUp' | 'arrowDown' | 'arrowLeft' | 'arrowRight' | 'home' | 'end'
      | 'pageUp' | 'pageDown' | 'insert' | 'delete' }>
  | Readonly<{ kind: 'character'; value: string }>
  | Readonly<{ kind: 'function'; value: number }>;

// Native keyboard/gesture implementations send this JSON directly to Rust.
// TypeScript controls use the same contract, never terminal escape sequences.
export type TerminalInput =
  | Readonly<{ type: 'commit' | 'preedit' | 'paste'; text: string }>
  | Readonly<{
      type: 'key'; key: PhysicalKey; action: 'press' | 'repeat' | 'release';
      modifiers?: ModifierBits; consumedModifiers?: ModifierBits;
      text?: string | null; unshiftedCodepoint?: number;
    }>
  | Readonly<{
      type: 'mouse'; action: 'press' | 'release' | 'motion';
      button?: 'left' | 'middle' | 'right' | 'wheelUp' | 'wheelDown'
        | 'wheelLeft' | 'wheelRight' | 'back' | 'forward' | null;
      modifiers?: ModifierBits; x: number; y: number;
      cellWidth: number; cellHeight: number; anyButtonPressed: boolean;
    }>;

const modifiers = z.number().int().min(0).max(15).optional();
const committedText = z.string().refine((value) => !/[\x00-\x1f\x7f-\x9f\uf700-\uf8ff]/u.test(value));
const physicalKey = z.union([
  z.object({ kind: z.enum(['unidentified', 'enter', 'tab', 'backspace', 'escape',
    'arrowUp', 'arrowDown', 'arrowLeft', 'arrowRight', 'home', 'end',
    'pageUp', 'pageDown', 'insert', 'delete']) }).strict(),
  z.object({ kind: z.literal('character'), value: z.string().regex(/^[ -~]$/) }).strict(),
  z.object({ kind: z.literal('function'), value: z.number().int().min(1).max(25) }).strict(),
]);
export const terminalInputSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('commit'), text: committedText }).strict(),
  z.object({ type: z.literal('preedit'), text: committedText }).strict(),
  z.object({ type: z.literal('paste'), text: z.string() }).strict(),
  z.object({
    type: z.literal('key'), key: physicalKey, action: z.enum(['press', 'repeat', 'release']),
    modifiers, consumedModifiers: modifiers, text: committedText.nullable().optional(),
    unshiftedCodepoint: z.number().int().min(0).max(0x10ffff)
      .refine((value) => value < 0xd800 || value > 0xdfff).optional(),
  }).strict(),
  z.object({
    type: z.literal('mouse'), action: z.enum(['press', 'release', 'motion']),
    button: z.enum(['left', 'middle', 'right', 'wheelUp', 'wheelDown', 'wheelLeft',
      'wheelRight', 'back', 'forward']).nullable().optional(), modifiers,
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
