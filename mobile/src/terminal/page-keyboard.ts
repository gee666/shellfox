/// <reference lib="dom" />
import type { Terminal } from '@xterm/xterm';
import { emptyModifiers, terminalKeySchema, type TerminalKeyEvent, type TerminalModifiers } from './protocol';

const namedCodes: Record<string, number> = {
  Backspace: 8, Tab: 9, Enter: 13, Escape: 27, PageUp: 33, PageDown: 34,
  End: 35, Home: 36, ArrowLeft: 37, ArrowUp: 38, ArrowRight: 39, ArrowDown: 40,
  Insert: 45, Delete: 46,
  F1: 112, F2: 113, F3: 114, F4: 115, F5: 116, F6: 117,
  F7: 118, F8: 119, F9: 120, F10: 121, F11: 122, F12: 123,
};
const punctuation = [';:', '=+', ',<', '-_', '.>', '/?', '`~', '[{', '\\|', ']}', '\'"'];
const punctuationCodes = [186, 187, 188, 189, 190, 191, 192, 219, 220, 221, 222];
const shiftedDigits = ')!@#$%^&*(';
function keyCode(key: string): number {
  if (namedCodes[key]) return namedCodes[key];
  const index = punctuation.findIndex(pair => pair.includes(key));
  if (index >= 0) return punctuationCodes[index]!;
  const digit = shiftedDigits.indexOf(key);
  return digit >= 0 ? 48 + digit : key.toUpperCase().charCodeAt(0);
}
function shiftedKey(key: string, modifiers: TerminalModifiers): string {
  if (/^[a-z]$/i.test(key)) return modifiers.shift !== modifiers.capsLock ? key.toUpperCase() : key.toLowerCase();
  if (!modifiers.shift) return key;
  if (/^[0-9]$/.test(key)) return shiftedDigits[Number(key)]!;
  return punctuation.find(pair => pair[0] === key)?.[1] ?? key;
}

/** Routes accessory keys through xterm's DOM encoder, including application cursor mode. */
export class PageKeyboard {
  private modifiers = { ...emptyModifiers };
  private synthetic = new WeakSet<KeyboardEvent>();
  private dispatching = false;
  private emitted = false;

  constructor(private readonly term: Terminal, private readonly emit: (data: string) => void) {
    term.attachCustomKeyEventHandler(event => {
      // xterm's ordinary keyup handler focuses the textarea. Accessory keys must not summon the IME.
      if (this.synthetic.has(event)) return event.type !== 'keyup';
      if (event.type !== 'keydown' || event.isComposing || event.keyCode === 229 || !this.active()) return true;
      if (!terminalKeySchema.safeParse(event.key).success) return true;
      event.preventDefault();
      this.key({ key: event.key, modifiers: {
        ctrl: event.ctrlKey || this.modifiers.ctrl, alt: event.altKey || this.modifiers.alt,
        shift: event.shiftKey || this.modifiers.shift, meta: event.metaKey || this.modifiers.meta,
        capsLock: event.getModifierState('CapsLock') !== this.modifiers.capsLock,
      } });
      return false;
    });
    term.onData(data => {
      if (this.dispatching) { this.emitted = true; this.emit(data); return; }
      // Mobile IMEs can emit text without keydown. Keep compositions/Unicode intact.
      if (this.active() && /^[\x20-\x7e]+$/.test(data)) {
        for (const key of data) this.key({ key, modifiers: this.modifiers });
      } else this.emit(data);
    });
  }
  setModifiers(modifiers: TerminalModifiers) { this.modifiers = modifiers; }
  paste(data: string) {
    this.dispatching = true;
    try { this.term.paste(data); } finally { this.dispatching = false; }
  }
  private active() { return Object.values(this.modifiers).some(Boolean); }

  key({ key, modifiers }: TerminalKeyEvent) {
    const textarea = this.term.textarea;
    if (!textarea) return;
    const value = key.length === 1 ? shiftedKey(key, modifiers) : key;
    const code = keyCode(key);
    const init: KeyboardEventInit = { key: value, keyCode: code, which: code, bubbles: true, cancelable: true,
      ctrlKey: modifiers.ctrl, altKey: modifiers.alt, shiftKey: modifiers.shift, metaKey: modifiers.meta };
    const dispatch = (type: string, options = init) => {
      const event = new KeyboardEvent(type, options);
      this.synthetic.add(event);
      textarea.dispatchEvent(event);
    };
    this.dispatching = true;
    this.emitted = false;
    try {
      dispatch('keydown');
      // xterm defers uppercase printable keys to keypress for macOS IME compatibility.
      if (!this.emitted && value.length === 1 && !modifiers.ctrl && !modifiers.alt && !modifiers.meta) {
        dispatch('keypress', { ...init, charCode: value.charCodeAt(0), which: value.charCodeAt(0) });
      }
      dispatch('keyup');
    } finally { this.dispatching = false; }
  }
}
