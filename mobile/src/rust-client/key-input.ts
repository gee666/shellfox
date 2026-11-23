import { terminalKeySchema, type TerminalKey, type TerminalModifiers } from '../terminal/protocol';
import type { KeyEvent, TerminalInput } from './input';

// Public GhosttyKey numbers (libghostty-vt 1.0.1 key/event.h).
const named: Readonly<Record<string, number>> = {
  Escape: 120, Tab: 64, Enter: 58, Backspace: 53,
  ArrowUp: 78, ArrowDown: 75, ArrowLeft: 76, ArrowRight: 77,
  Home: 71, End: 69, PageUp: 74, PageDown: 73, Insert: 72, Delete: 68,
};
// US key positions: unshifted character, its shifted one, and the key number.
const positions: ReadonlyArray<readonly [string, string, number]> = [
  ['`', '~', 1], ['\\', '|', 2], ['[', '{', 3], [']', '}', 4], [',', '<', 5], ['=', '+', 16],
  ['-', '_', 46], ['.', '>', 47], ['\'', '"', 48], [';', ':', 49], ['/', '?', 50], [' ', ' ', 63],
  ...Array.from(')!@#$%^&*(', (shifted, digit) => [String(digit), shifted, 6 + digit] as const),
];
const SHIFT = 1, CTRL = 2, ALT = 4, SUPER = 8, CAPS = 16;

export function modifierBits(modifiers: TerminalModifiers): number {
  return Number(modifiers.shift) * SHIFT | Number(modifiers.ctrl) * CTRL
    | Number(modifiers.alt) * ALT | Number(modifiers.meta) * SUPER;
}

/** One key-bar key as the key press Ghostty encodes. */
export function keyInput(key: TerminalKey, modifiers: TerminalModifiers): TerminalInput {
  terminalKeySchema.parse(key);
  const bits = modifierBits(modifiers);
  const press = (physical: number, text = '', unshifted: number | null = null, consumed = 0): TerminalInput => ({
    type: 'key', event: { physical, action: 'press', modifiers: bits | (modifiers.capsLock ? CAPS : 0),
      consumedModifiers: consumed, text, unshiftedCodepoint: unshifted, composing: false },
  });
  const special = named[key];
  if (special !== undefined) return press(special);
  if (/^F\d+$/.test(key)) return press(120 + Number(key.slice(1)));
  if (/^[a-z]$/i.test(key)) {
    const base = key.toLowerCase();
    const text = modifiers.shift !== modifiers.capsLock ? base.toUpperCase() : base;
    return press(20 + base.charCodeAt(0) - 97, text, base.charCodeAt(0), modifiers.shift ? SHIFT : 0);
  }
  const position = positions.find(([plain, shifted]) => plain === key || shifted === key);
  if (!position) throw new Error(`No key for ${key}.`);
  const [plain, shifted, physical] = position;
  const text = modifiers.shift || (key === shifted && key !== plain) ? shifted : plain;
  return press(physical, text, plain.charCodeAt(0), modifiers.shift && text !== plain ? SHIFT : 0);
}

/** The release for a key press: same key and modifiers, no text. */
export function keyRelease(input: TerminalInput): TerminalInput {
  if (input.type !== 'key') throw new Error('Only key presses have releases.');
  const event: KeyEvent = { ...input.event, action: 'release', text: '' };
  return { type: 'key', event };
}
