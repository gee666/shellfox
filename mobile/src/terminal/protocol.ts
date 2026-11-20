import { z } from 'zod';

export const utf8Size = (text: string): number => new TextEncoder().encode(text).byteLength;
export const terminalFontSize = (size: number): number => Number.isFinite(size) ? Math.max(8, Math.min(32, size)) : 14;

export interface TerminalModifiers {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  meta: boolean;
  capsLock: boolean;
}
export const emptyModifiers: TerminalModifiers = { ctrl: false, alt: false, shift: false, meta: false, capsLock: false };
export const terminalKeySchema = z.union([
  z.enum(['Escape', 'Tab', 'Enter', 'Backspace', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight',
    'Home', 'End', 'PageUp', 'PageDown', 'Insert', 'Delete',
    'F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8', 'F9', 'F10', 'F11', 'F12']),
  z.string().length(1).regex(/^[\x20-\x7e]$/),
]);
export type TerminalKey = z.infer<typeof terminalKeySchema>;
