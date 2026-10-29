// @vitest-environment jsdom
import { readFileSync } from 'node:fs';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { NativeTextInput, terminalTextEdit } from './native-text-input';

it('keeps the mobile text handler out of the desktop terminal', () => {
  const desktop = readFileSync('src/renderer/terminal-surface.ts', 'utf8');
  expect(desktop).not.toMatch(/\b(?:TerminalInput|NativeTextInput)\b|(?:native-text-input|terminal-input)/);
});

const state = (value: string, cursor = value.length) => ({ value, cursor });
afterEach(() => { vi.useRealTimers(); document.body.replaceChildren(); });
describe('terminal textarea edits', () => {
  it.each(['one more thing ', '日本語🙂', 'e\u0301', '11'])('preserves literal Unicode and intentional repetition: %s', text => {
    expect(terminalTextEdit(state(text), state(text + text))).toBe(text);
    expect(terminalTextEdit(state(text), state(text))).toBe('');
  });
  it('replaces the changed range and restores the cursor rather than reinserting context', () => {
    expect(terminalTextEdit(state('there is one more thing'), state('there is one more Thing')))
      .toBe('\x1b[D'.repeat(4) + '\x7fT' + '\x1b[C'.repeat(4));
    expect(terminalTextEdit(state('abc'), state('ac', 1), true)).toBe('\x1bOD\x7f');
    expect(terminalTextEdit(state('phrase'), state('phras'))).toBe('\x7f');
  });
  it('does not split an emoji or a combining sequence while replacing it', () => {
    expect(terminalTextEdit(state('x🙂'), state('x🙃'))).toBe('\x7f🙃');
    expect(terminalTextEdit(state('cafe\u0301'), state('café'))).toBe('\x7fé');
  });
});

function fixture(disableStdin = false) {
  const element = document.createElement('div'), textarea = document.createElement('textarea');
  element.append(textarea); document.body.append(element);
  const callbacks = { text: vi.fn<(data: string) => void>(), start: vi.fn<() => void>(), commit: vi.fn<(data: string) => void>() };
  let encodedKey: (event: { key: string; domEvent: KeyboardEvent }) => void = () => {};
  const unsubscribe = vi.fn();
  const terminal = { element, textarea, options: { disableStdin }, modes: { applicationCursorKeysMode: false },
    onKey: (listener: typeof encodedKey) => { encodedKey = listener; return { dispose: unsubscribe }; },
    paste: vi.fn((_data: string) => { textarea.value = ''; }) };
  const input = new NativeTextInput(terminal, callbacks);
  const edit = (text: string) => {
    textarea.dispatchEvent(new InputEvent('beforeinput', { bubbles: true, inputType: 'insertText', data: text }));
    textarea.setRangeText(text, textarea.selectionStart, textarea.selectionEnd, 'end');
    textarea.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: text }));
  };
  return { ...terminal, input, callbacks, edit, unsubscribe, encodeKey: (event: KeyboardEvent) => encodedKey({ key: event.key, domEvent: event }) };
}
describe('native DOM input ownership', () => {
  it('cancels the default edit only for keypresses encoded by xterm', async () => {
    const f = fixture();
    for (const shiftKey of [false, true]) {
      const event = new KeyboardEvent('keypress', { key: 'A', shiftKey, cancelable: true });
      f.encodeKey(event);
      expect(event.defaultPrevented).toBe(true);
    }
    const nativeKey = new KeyboardEvent('keypress', { key: 'é', cancelable: true });
    f.textarea.dispatchEvent(nativeKey);
    expect(nativeKey.defaultPrevented).toBe(false);
    f.edit('é'); f.edit('é'); await Promise.resolve();
    expect(f.callbacks.text.mock.calls).toEqual([['é'], ['é']]);
    f.input.dispose(); expect(f.unsubscribe).toHaveBeenCalledOnce();
  });
  it('prevents 229 fallback timers, not native editing, and emits every DOM transaction once', async () => {
    const f = fixture(), fallback = vi.fn(); f.textarea.addEventListener('keydown', fallback);
    for (let i = 0; i < 2; i++) {
      const key = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, keyCode: 229 });
      f.textarea.dispatchEvent(key); expect(key.defaultPrevented).toBe(false);
    }
    f.edit('one more thing '); f.edit('one more thing '); await Promise.resolve();
    expect(fallback).not.toHaveBeenCalled();
    expect(f.callbacks.text.mock.calls).toEqual([['one more thing '], ['one more thing ']]);
    f.input.dispose();
  });
  it('does not emit edits while terminal input is disabled or after disposal', async () => {
    const f = fixture(true); f.edit('blocked'); await Promise.resolve(); expect(f.callbacks.text).toHaveBeenCalledTimes(0);
    f.input.dispose(); f.options.disableStdin = false; f.edit('retired'); await Promise.resolve();
    expect(f.callbacks.text).toHaveBeenCalledTimes(0);
  });
  it('retires pending composition commits on disposal', async () => {
    vi.useFakeTimers(); const f = fixture();
    f.textarea.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
    f.edit('日本語'); f.textarea.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }));
    expect(f.input.pendingCommit).toBe(true); f.input.dispose();
    await vi.runAllTimersAsync(); expect(f.callbacks.commit).not.toHaveBeenCalled();
  });
});
