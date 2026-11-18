import { terminalKeySchema, type TerminalKey, type TerminalModifiers } from '../terminal/protocol';
import type { PhysicalKey, TerminalInput } from './input';

const named: Readonly<Record<string, PhysicalKey>> = {
  Escape: { kind: 'escape' }, Tab: { kind: 'tab' }, Enter: { kind: 'enter' }, Backspace: { kind: 'backspace' },
  ArrowUp: { kind: 'arrowUp' }, ArrowDown: { kind: 'arrowDown' }, ArrowLeft: { kind: 'arrowLeft' }, ArrowRight: { kind: 'arrowRight' },
  Home: { kind: 'home' }, End: { kind: 'end' }, PageUp: { kind: 'pageUp' }, PageDown: { kind: 'pageDown' },
  Insert: { kind: 'insert' }, Delete: { kind: 'delete' },
};
const punctuation = [';:', '=+', ',<', '-_', '.>', '/?', '`~', '[{', '\\|', ']}', '\'"'];
const shiftedDigits = ')!@#$%^&*(';
export function modifierBits(modifiers: TerminalModifiers): number {
  return Number(modifiers.shift) | Number(modifiers.ctrl) << 1
    | Number(modifiers.alt) << 2 | Number(modifiers.meta) << 3;
}

/** Physical identity and layout text only. Ghostty produces the terminal bytes. */
export function keyInput(key: TerminalKey, modifiers: TerminalModifiers): TerminalInput {
  terminalKeySchema.parse(key);
  const bits = modifierBits(modifiers);
  const identity = named[key];
  if (identity) return { type: 'key', key: identity, action: 'press', modifiers: bits };
  if (/^F\d+$/.test(key)) return {
    type: 'key', key: { kind: 'function', value: Number(key.slice(1)) }, action: 'press', modifiers: bits,
  };
  let text = key;
  if (/^[a-z]$/i.test(key)) text = modifiers.shift !== modifiers.capsLock ? key.toUpperCase() : key.toLowerCase();
  else if (modifiers.shift) {
    text = /^[0-9]$/.test(key) ? shiftedDigits[Number(key)] ?? key
      : punctuation.find((pair) => pair[0] === key)?.[1] ?? key;
  }
  const digit = shiftedDigits.indexOf(key);
  const position = punctuation.find((pair) => pair.includes(key))?.[0]
    ?? (digit >= 0 ? String(digit) : key.toLowerCase());
  return { type: 'key', key: { kind: 'character', value: position }, action: 'press',
    modifiers: bits, consumedModifiers: modifiers.shift ? 1 : 0,
    text, unshiftedCodepoint: position.codePointAt(0) ?? 0,
  };
}
