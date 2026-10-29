/// <reference lib="dom" />
import type { Terminal } from '@xterm/xterm';
import { NativeTextInput } from './native-text-input';
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
const ascii = (data: string) => /^[\x20-\x7e]+$/.test(data);
const active = (modifiers: TerminalModifiers) => Object.values(modifiers).some(Boolean);
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

/** Native text has one DOM edit owner; accessory keys use xterm's public key encoder. */
export class PageKeyboard {
  private modifiers = { ...emptyModifiers };
  private compositionModifiers: TerminalModifiers | null = null;
  private readonly nativeInput: NativeTextInput;
  private readonly synthetic = new WeakSet<KeyboardEvent>();
  private dispatching = false;
  private emitted = false;
  private encodedKey = false;
  private nativeKeyOutput = false;
  private userInput = false;
  private disposed = false;
  private pendingActions: (() => void)[] = [];
  private readonly cleanups: (() => void)[] = [];

  constructor(private readonly term: Terminal, private readonly emit: (data: string, userInitiated?: boolean) => void) {
    // Device replies also use onData. Keep real DOM input, including mouse
    // reports, distinct from parser output even while a write is in progress.
    const element = term.element;
    const inputEvents = ['keydown', 'keypress', 'input', 'compositionend', 'paste', 'mousedown', 'mouseup', 'mousemove', 'wheel'];
    const markUserInput = () => { this.userInput = true; queueMicrotask(() => { this.userInput = false; }); };
    for (const type of inputEvents) element?.addEventListener(type, markUserInput, true);
    this.cleanups.push(() => { for (const type of inputEvents) element?.removeEventListener(type, markUserInput, true); });
    this.nativeInput = new NativeTextInput(term, {
      start: () => { this.compositionModifiers = { ...this.modifiers }; },
      commit: data => {
        const modifiers = this.compositionModifiers ?? this.modifiers;
        this.compositionModifiers = null;
        this.text(data, modifiers);
        const pending = this.pendingActions; this.pendingActions = [];
        for (const action of pending) action();
      },
      text: data => this.text(data, this.modifiers),
    });
    term.attachCustomKeyEventHandler(event => {
      if (this.disposed) return false;
      // xterm's keyup handler focuses the textarea. Accessory keys must not summon the IME.
      if (this.synthetic.has(event)) return event.type !== 'keyup';
      if (event.type !== 'keydown' || event.isComposing || event.keyCode === 229 || !active(this.modifiers)) return true;
      if (!terminalKeySchema.safeParse(event.key).success) return true;
      event.preventDefault();
      this.key({ key: event.key, modifiers: {
        ctrl: event.ctrlKey || this.modifiers.ctrl, alt: event.altKey || this.modifiers.alt,
        shift: event.shiftKey || this.modifiers.shift, meta: event.metaKey || this.modifiers.meta,
        capsLock: event.getModifierState('CapsLock') !== this.modifiers.capsLock,
      } });
      return false;
    });
    const keySubscription = term.onKey(() => {
      if (this.dispatching) this.encodedKey = true;
      else {
        this.nativeKeyOutput = true;
        queueMicrotask(() => { this.nativeKeyOutput = false; });
      }
    });
    const subscription = term.onData(data => {
      if (this.disposed) return;
      if (this.dispatching && this.encodedKey || this.nativeKeyOutput || this.nativeInput.acceptsXtermData(data)) {
        this.emitted = true;
        if (this.dispatching && this.encodedKey || this.nativeKeyOutput || this.userInput) this.emit(data);
        else this.emit(data, false);
      }
    });
    this.cleanups.push(() => keySubscription.dispose(), () => subscription.dispose());
  }
  private text(data: string, modifiers: TerminalModifiers) {
    if (this.disposed || !data) return;
    if (active(modifiers) && ascii(data)) {
      for (const key of data) this.dispatchKey({ key, modifiers });
    } else this.emit(data);
  }
  setModifiers(modifiers: TerminalModifiers) { this.modifiers = { ...modifiers }; }
  paste(data: string) {
    if (this.disposed) return;
    if (this.nativeInput.pendingCommit) { this.pendingActions.push(() => this.paste(data)); return; }
    this.nativeInput.finish();
    this.dispatching = true; this.encodedKey = true;
    try { this.term.paste(data); } finally { this.dispatching = false; }
  }
  key(event: TerminalKeyEvent) {
    if (this.disposed) return;
    if (this.nativeInput.pendingCommit) {
      const copy = { key: event.key, modifiers: { ...event.modifiers } };
      this.pendingActions.push(() => this.key(copy)); return;
    }
    this.nativeInput.finish();
    this.dispatchKey(event);
  }
  dispose() {
    this.disposed = true; this.nativeInput.dispose(); this.compositionModifiers = null; this.pendingActions = [];
    for (const cleanup of this.cleanups) cleanup();
  }
  private dispatchKey({ key, modifiers }: TerminalKeyEvent) {
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
    this.dispatching = true; this.encodedKey = false; this.emitted = false;
    try {
      dispatch('keydown');
      if (!this.emitted && value.length === 1 && !modifiers.ctrl && !modifiers.alt && !modifiers.meta) {
        dispatch('keypress', { ...init, charCode: value.charCodeAt(0), which: value.charCodeAt(0) });
      }
      dispatch('keyup');
    } finally { this.dispatching = false; }
  }
}
