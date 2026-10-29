import type { Terminal } from '@xterm/xterm';

interface TextState { value: string; cursor: number; start?: number }
interface Composition { before: TextState; edited: boolean; timer?: ReturnType<typeof setTimeout> }
type TextTerminal = Pick<Terminal, 'textarea' | 'element' | 'paste' | 'onKey'> & {
  options: Pick<Terminal['options'], 'disableStdin'>;
  modes: Pick<Terminal['modes'], 'applicationCursorKeysMode'>;
};
interface InputCallbacks {
  text(data: string): void;
  start(): void;
  commit(data: string): void;
}
interface GraphemeSegmenter { segment(text: string): Iterable<{ segment: string }> }
const intl: typeof Intl & { Segmenter?: new (locale: undefined, options: { granularity: 'grapheme' }) => GraphemeSegmenter } = Intl;
const segmenter = typeof intl.Segmenter === 'function' ? new intl.Segmenter(undefined, { granularity: 'grapheme' }) : null;
const characters = (text: string) => segmenter ? Array.from(segmenter.segment(text), part => part.segment) : Array.from(text);

/** Translate an actual textarea edit, not its whole retained IME context, to terminal input. */
function textMutation(before: TextState, after: TextState, applicationCursor = false) {
  const empty = { before: '', data: '', after: '' };
  if (before.value === after.value) return empty;
  if (before.cursor === before.value.length && after.cursor === after.value.length && after.value.startsWith(before.value)) {
    return { ...empty, data: after.value.slice(before.value.length) };
  }
  const old = characters(before.value), next = characters(after.value);
  let prefix = 0, suffix = 0;
  while (prefix < old.length && prefix < next.length && old[prefix] === next[prefix]) prefix++;
  while (suffix < old.length - prefix && suffix < next.length - prefix && old[old.length - 1 - suffix] === next[next.length - 1 - suffix]) suffix++;
  const oldEnd = old.length - suffix, newEnd = next.length - suffix;
  const cursor = (state: TextState) => characters(state.value.slice(0, state.cursor)).length;
  const move = (from: number, to: number) => (applicationCursor ? '\x1bO' : '\x1b[').concat(to < from ? 'D' : 'C').repeat(Math.abs(to - from));
  return { before: move(cursor(before), oldEnd) + '\x7f'.repeat(oldEnd - prefix), data: next.slice(prefix, newEnd).join(''), after: move(newEnd, cursor(after)) };
}
export function terminalTextEdit(before: TextState, after: TextState, applicationCursor = false): string {
  const edit = textMutation(before, after, applicationCursor);
  return edit.before + edit.data + edit.after;
}

/** Own native DOM text edits. xterm still owns key encoding, paste and composition display.
 * Its keyCode 229 fallback schedules one textarea-diff timer per keydown, which can
 * replay a commit twice or insert the entire retained buffer during autocorrect. */
export class TerminalInput {
  private composition: Composition | null = null;
  private before: TextState | null = null;
  private pendingEdit: { before: TextState; after: TextState; paste: boolean } | null = null;
  private committed: TextState;
  private forced = false;
  private finalizing = false;
  private pasting = false;
  private disposed = false;
  private readonly cleanups: (() => void)[] = [];
  private readonly textarea: HTMLTextAreaElement;

  constructor(private readonly term: TextTerminal, private readonly callbacks: InputCallbacks) {
    const textarea = term.textarea, element = term.element;
    if (!textarea || !element) throw new Error('Open the terminal before installing native input.');
    this.textarea = textarea;
    this.committed = this.snapshot();
    const encodedKeys = term.onKey(({ domEvent }) => {
      // xterm emits uppercase letters on keypress without cancelling its default
      // edit. Do not send that same letter again through the native input path.
      if (domEvent.type === 'keypress') domEvent.preventDefault();
    });
    this.cleanups.push(() => encodedKeys.dispose());
    const listen = (type: string, listener: EventListener, capture = true) => {
      element.addEventListener(type, listener, capture);
      this.cleanups.push(() => element.removeEventListener(type, listener, capture));
    };
    listen('keydown', event => {
      if (!(event instanceof KeyboardEvent) || event.target !== textarea || this.finalizing) return;
      this.flushEdit();
      if (event.keyCode === 229) {
        // Do not cancel the browser edit. Only prevent xterm's timer-based fallback.
        event.stopImmediatePropagation();
      } else if (![16, 17, 18, 20].includes(event.keyCode)) {
        this.finish();
        // xterm clears native context for Enter/Ctrl+C after its key handler runs.
        queueMicrotask(() => { if (!this.composition && !this.disposed) { this.committed = this.snapshot(); this.before = null; } });
      }
    });
    listen('beforeinput', event => {
      if (event.target !== textarea) return;
      this.flushEdit();
      if (this.composition) this.composition.edited = true;
      this.before = this.snapshot();
    });
    listen('compositionstart', event => {
      if (event.target !== textarea) return;
      this.flushEdit(); this.finish(); this.forced = false;
      this.composition = { before: this.snapshot(), edited: false }; this.before = null;
      this.callbacks.start();
    });
    listen('compositionupdate', event => {
      if (event.target === textarea && this.forced) event.stopImmediatePropagation();
    });
    listen('compositionend', event => {
      if (event.target === textarea && this.forced) {
        this.forced = false; this.committed = this.snapshot(); this.before = null;
        event.stopImmediatePropagation();
      }
    });
    // xterm's textarea listener schedules its finalizer first. Read the final DOM
    // value afterwards, including the native input event following compositionend.
    listen('compositionend', event => {
      if (event.target !== textarea || !this.composition) return;
      const composition = this.composition;
      composition.timer = setTimeout(() => {
        if (this.composition === composition && !this.disposed) this.commit(composition);
      }, 0);
    }, false);
    listen('input', event => {
      if (event.target !== textarea || !(event instanceof InputEvent)) return;
      event.stopImmediatePropagation();
      const after = this.snapshot(), before = this.before ?? this.committed;
      const start = before.start ?? before.cursor;
      const inserted = event.data ?? '';
      if (before.value.slice(0, start) + inserted + before.value.slice(before.cursor) === after.value && before.value !== after.value) {
        // Chromium's programmatic text insertion can leave the pre-edit selection
        // in place. The beforeinput range gives the resulting caret exactly.
        after.cursor = start + inserted.length;
        textarea.setSelectionRange(after.cursor, after.cursor);
      }
      if (this.composition) { this.composition.edited = true; this.before = null; return; }
      // beforeinput may select an old word for replacement without moving the
      // terminal cursor. Keep the last committed cursor when its value still matches.
      const cursor = before.value === this.committed.value ? this.committed.cursor : before.cursor;
      this.before = null;
      const edit = { before: { ...before, cursor }, after, paste: event.inputType === 'insertFromPaste' }; this.pendingEdit = edit;
      // Chromium can update the caret after input capture has run. Finish this
      // edit at the end of DOM dispatch, or before the next edit starts.
      queueMicrotask(() => { if (this.pendingEdit === edit && !this.disposed) this.flushEdit(); });
    });
    listen('paste', event => {
      if (!(event instanceof ClipboardEvent) || !event.clipboardData) return;
      // Cancel the browser edit so paste cannot also arrive as a native input.
      event.preventDefault(); event.stopImmediatePropagation();
      this.finish();
      if (!this.term.options.disableStdin) this.paste(event.clipboardData.getData('text/plain'));
    });
    listen('blur', () => { queueMicrotask(() => { if (!this.composition) this.committed = this.snapshot(); }); });
  }
  private snapshot(): TextState { return { value: this.textarea.value, cursor: this.textarea.selectionEnd, start: this.textarea.selectionStart }; }
  private flushEdit() {
    const edit = this.pendingEdit;
    if (!edit || this.disposed) return;
    this.pendingEdit = null;
    const current = this.snapshot();
    const after = current.value === edit.after.value ? current : edit.after;
    this.committed = after;
    if (this.term.options.disableStdin) return;
    if (edit.paste) {
      const mutation = textMutation(edit.before, after, this.term.modes.applicationCursorKeysMode);
      if (mutation.before) this.callbacks.text(mutation.before);
      this.paste(mutation.data);
      if (mutation.after) this.callbacks.text(mutation.after);
    } else this.callbacks.text(terminalTextEdit(edit.before, after, this.term.modes.applicationCursorKeysMode));
  }
  private paste(data: string) {
    this.pasting = true;
    try { this.term.paste(data); }
    finally { this.pasting = false; this.committed = this.snapshot(); }
  }
  private commit(composition: Composition) {
    if (composition.timer) clearTimeout(composition.timer);
    const after = this.snapshot();
    this.composition = null; this.before = null; this.committed = after;
    // Without a native edit event, a shorter value is a cancelled preedit, not
    // authority to erase previously committed terminal text.
    const cancelled = !composition.edited && after.value.length < composition.before.value.length;
    this.callbacks.commit(this.term.options.disableStdin || cancelled ? '' : terminalTextEdit(composition.before, after, this.term.modes.applicationCursorKeysMode));
  }
  get pendingCommit(): boolean { return this.composition?.timer !== undefined; }
  /** Finish before an accessory/hardware key or paste, without waiting for IME dismissal. */
  finish() {
    if (this.disposed || this.finalizing) return;
    this.flushEdit();
    const composition = this.composition;
    if (!composition) return;
    this.forced = composition.timer === undefined;
    this.finalizing = true;
    try {
      // A public, unencoded key lets xterm retire its own composition display/timer.
      this.textarea.dispatchEvent(new KeyboardEvent('keydown', { key: 'Unidentified', keyCode: 0, bubbles: true }));
    } finally { this.finalizing = false; }
    this.commit(composition);
  }
  /** Native text is emitted by the edit transaction, never again by xterm's finalizer. */
  acceptsXtermData(data: string): boolean {
    return this.pasting || !this.composition && !this.finalizing && !this.forced || data.startsWith('\x1b');
  }
  dispose() {
    this.disposed = true;
    if (this.composition?.timer) clearTimeout(this.composition.timer);
    this.composition = null; this.pendingEdit = null;
    for (const cleanup of this.cleanups) cleanup();
  }
}
