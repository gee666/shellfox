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
interface Composition {
  type: 'text'; data: string; modifiers: TerminalModifiers; ready: boolean;
  start?: number; boundary?: number;
}
type Pending = Composition | { type: 'key'; event: TerminalKeyEvent } | { type: 'paste'; data: string } | { type: 'literal'; data: string };

/** Routes accessory keys through xterm's DOM encoder, including application cursor mode. */
export class PageKeyboard {
  private modifiers = { ...emptyModifiers };
  private synthetic = new WeakSet<KeyboardEvent>();
  private dispatching = false;
  private emitted = false;
  private composing: Composition | null = null;
  private ending: Composition[] = [];
  private provisionalInput = false;
  private pending: Pending[] = [];
  private timers = new Set<ReturnType<typeof setTimeout>>();
  private disposed = false;
  private encodedKey = false;
  private nativeKeyOutput = false;
  private draining = false;
  private forcedNativeComposition = false;
  private forcedCommit: Composition | null = null;
  private readonly cleanups: (() => void)[] = [];

  constructor(private readonly term: Terminal, private readonly emit: (data: string) => void) {
    term.attachCustomKeyEventHandler(event => {
      if (this.disposed) return false;
      // xterm's ordinary keyup handler focuses the textarea. Accessory keys must not summon the IME.
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
    const textarea = term.textarea, element = term.element;
    if (textarea && element) {
      const start = () => {
        this.forcedNativeComposition = false; this.forcedCommit = null;
        const previous = this.ending.at(-1);
        if (previous?.start !== undefined) previous.boundary = Math.max(0, textarea.value.length - previous.start);
        const composition: Composition = { type: 'text', data: '', modifiers: { ...this.modifiers }, ready: false, start: textarea.value.length };
        this.composing = composition; this.pending.push(composition);
      };
      const end = () => this.finishComposition(false);
      const compositionEvent = (event: Event) => {
        // An accessory key already made xterm finalize this composition. Do not feed its
        // eventual native end back into CompositionHelper, which would send the text again.
        if (this.forcedNativeComposition) {
          event.stopImmediatePropagation();
          if (event.type === 'compositionend') {
            this.forcedNativeComposition = false;
            const commit = this.forcedCommit;
            this.later(() => { if (this.forcedCommit === commit) this.forcedCommit = null; });
          }
        }
      };
      const keydown = (event: KeyboardEvent) => {
        // The sticky handler routes known keys through key(), which must finish composition
        // itself. Other hardware keys go directly to xterm's CompositionHelper.
        const routed = !event.isComposing && active(this.modifiers) && terminalKeySchema.safeParse(event.key).success;
        if (this.composing && !routed && ![16, 17, 18, 20, 229].includes(event.keyCode)) this.finishComposition(true);
      };
      const input = (event: Event) => {
        if (event.target !== textarea || !(event instanceof InputEvent) || event.inputType !== 'insertText') return;
        if (this.forcedCommit?.data && event.data === this.forcedCommit.data) {
          event.stopImmediatePropagation();
          return;
        }
        if (!this.composing && !this.ending.length) return;
        // xterm can emit insertText provisionally during composition. Identify that DOM dispatch,
        // not every onData while composing: a prior composition can commit during the next one.
        this.provisionalInput = true;
        queueMicrotask(() => { this.provisionalInput = false; });
      };
      textarea.addEventListener('compositionstart', start);
      textarea.addEventListener('compositionend', end);
      element.addEventListener('input', input, true);
      element.addEventListener('keydown', keydown, true);
      element.addEventListener('compositionupdate', compositionEvent, true);
      element.addEventListener('compositionend', compositionEvent, true);
      this.cleanups.push(() => {
        textarea.removeEventListener('compositionstart', start);
        textarea.removeEventListener('compositionend', end);
        element.removeEventListener('input', input, true);
        element.removeEventListener('keydown', keydown, true);
        element.removeEventListener('compositionupdate', compositionEvent, true);
        element.removeEventListener('compositionend', compositionEvent, true);
      });
    }
    const keySubscription = term.onKey(() => {
      if (this.dispatching) this.encodedKey = true;
      else {
        this.nativeKeyOutput = true;
        queueMicrotask(() => { this.nativeKeyOutput = false; });
      }
    });
    this.cleanups.push(() => keySubscription.dispose());
    const subscription = term.onData(data => {
      if (this.disposed) return;
      if ((this.dispatching && this.encodedKey) || this.nativeKeyOutput) {
        this.emitted = true;
        if (this.pending.length && !this.draining) this.pending.push({ type: 'literal', data });
        else this.emit(data);
        return;
      }
      if (this.provisionalInput && !/[\x00-\x1f\x7f]/.test(data)) return;
      const composition = this.ending[0];
      if (composition && !this.provisionalInput && !/[\x00-\x1f\x7f]/.test(data)) {
        // xterm may combine two completed sessions into one emission before its timer runs.
        // The next composition's textarea offset gives the boundary without trusting event.data.
        let remaining = data;
        for (const item of this.ending) {
          const count = item.boundary === undefined ? remaining.length : Math.min(Math.max(0, item.boundary - item.data.length), remaining.length);
          item.data += remaining.slice(0, count);
          remaining = remaining.slice(count);
          if (!remaining) break;
        }
        return;
      }
      if (this.dispatching) { this.emitted = true; this.emit(data); return; }
      if (active(this.modifiers) && ascii(data)) {
        this.pending.push({ type: 'text', data, modifiers: { ...this.modifiers }, ready: true });
        // Never re-enter CompositionHelper from onData, even for non-composing IME input.
        this.later(() => this.drain());
      } else this.emit(data);
    });
    this.cleanups.push(() => subscription.dispose());
  }
  private finishComposition(forced: boolean) {
    const composition = this.composing;
    if (!composition) return;
    this.composing = null; this.ending.push(composition);
    this.forcedNativeComposition = forced;
    if (forced) { this.provisionalInput = false; this.forcedCommit = composition; }
    // Native end schedules xterm's final commit first. A terminating key commits synchronously.
    this.later(() => {
      composition.ready = true;
      this.ending = this.ending.filter(item => item !== composition);
      this.drain();
    });
  }
  private later(action: () => void) {
    const timer = setTimeout(() => {
      this.timers.delete(timer);
      if (!this.disposed) action();
    }, 0);
    this.timers.add(timer);
  }
  setModifiers(modifiers: TerminalModifiers) { this.modifiers = { ...modifiers }; }
  paste(data: string) {
    if (this.disposed) return;
    if (this.composing) {
      this.finishComposition(true);
      // A key with no terminal encoding lets xterm finish its own composition before paste.
      this.dispatchKey({ key: 'Unidentified', modifiers: { ...emptyModifiers } });
    }
    if (this.pending.length) { this.pending.push({ type: 'paste', data }); return; }
    this.dispatchPaste(data);
  }
  private dispatchPaste(data: string) {
    this.dispatching = true; this.encodedKey = true;
    try { this.term.paste(data); } finally { this.dispatching = false; }
  }
  key(event: TerminalKeyEvent) {
    if (this.disposed) return;
    if (this.composing) {
      this.finishComposition(true);
      this.dispatchKey(event);
    } else if (this.pending.length) {
      this.pending.push({ type: 'key', event: { key: event.key, modifiers: { ...event.modifiers } } });
    } else this.dispatchKey(event);
  }
  private drain() {
    if (this.disposed || this.composing) return;
    this.draining = true;
    try {
      while (this.pending.length) {
        const item = this.pending[0]!;
        if (item.type === 'text' && !item.ready) return;
        this.pending.shift();
        if (item.type === 'key') this.dispatchKey(item.event);
        else if (item.type === 'paste') this.dispatchPaste(item.data);
        else if (item.type === 'literal') this.emit(item.data);
        else if (item.data) {
          if (active(item.modifiers) && ascii(item.data)) {
            for (const key of item.data) this.dispatchKey({ key, modifiers: item.modifiers });
          } else this.emit(item.data);
        }
      }
    } finally { this.draining = false; }
  }
  dispose() {
    this.disposed = true;
    for (const timer of this.timers) clearTimeout(timer);
    this.timers.clear(); this.pending = []; this.ending = []; this.composing = null; this.forcedCommit = null;
    for (const cleanup of this.cleanups) cleanup();
  }
  private dispatchKey({ key, modifiers }: { key: string; modifiers: TerminalModifiers }) {
    const textarea = this.term.textarea;
    if (!textarea) return;
    const value = key.length === 1 ? shiftedKey(key, modifiers) : key;
    const code = key === 'Unidentified' ? 0 : keyCode(key);
    const init: KeyboardEventInit = { key: value, keyCode: code, which: code, bubbles: true, cancelable: true,
      ctrlKey: modifiers.ctrl, altKey: modifiers.alt, shiftKey: modifiers.shift, metaKey: modifiers.meta };
    const dispatch = (type: string, options = init) => {
      const event = new KeyboardEvent(type, options);
      this.synthetic.add(event);
      textarea.dispatchEvent(event);
    };
    this.dispatching = true; this.encodedKey = false;
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
