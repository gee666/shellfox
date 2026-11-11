// @vitest-environment jsdom
import { cleanup, fireEvent, render } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { layoutShortcut, layoutShortcutHint, useLayoutShortcuts } from './layout-shortcuts';

afterEach(() => { cleanup(); vi.restoreAllMocks(); });
const key = (code: string, options: KeyboardEventInit = {}) => new KeyboardEvent('keydown', { code, ctrlKey: true, cancelable: true, bubbles: true, ...options });

describe('layout shortcuts', () => {
  it.each([false, true])('matches the client modifier, mac=%s', mac => {
    const modifier = { ctrlKey: !mac, metaKey: mac };
    expect(layoutShortcut(key('KeyB', modifier), mac)).toBe('sessions');
    expect(layoutShortcut(key('KeyB', { ...modifier, shiftKey: true }), mac)).toBe('files');
    expect(layoutShortcut(key('KeyE', { ...modifier, shiftKey: true }), mac)).toBe('folders');
    expect(layoutShortcut(key('KeyE', modifier), mac)).toBeNull();
    expect(layoutShortcut(key('KeyB', { ...modifier, altKey: true }), mac)).toBeNull();
    expect(layoutShortcut(key('KeyB', { ...modifier, isComposing: true }), mac)).toBeNull();
    expect(layoutShortcut(key('KeyB', { ctrlKey: true, metaKey: true }), mac)).toBeNull();
    expect(layoutShortcut(key('KeyB', { ctrlKey: mac, metaKey: !mac }), mac)).toBeNull();
    expect(layoutShortcut(key('', { ...modifier, key: 'b' }), mac)).toBe('sessions');
  });
  it('uses platform-specific hints', () => {
    expect(layoutShortcutHint('sessions', false)).toBe('Ctrl+B');
    expect(layoutShortcutHint('files', false)).toBe('Ctrl+Shift+B');
    expect(layoutShortcutHint('folders', false)).toBe('Ctrl+Shift+E');
    expect(layoutShortcutHint('sessions', true)).toBe('⌘B');
    expect(layoutShortcutHint('files', true)).toBe('⌘⇧B');
    expect(layoutShortcutHint('folders', true)).toBe('⌘⇧E');
  });
});

function Shortcuts({ run }: { run: (action: string) => boolean }) {
  useLayoutShortcuts(run);
  return <textarea aria-label="Terminal input" />;
}

it('consumes the full key sequence before input handlers, toggles once and cleans up', () => {
  const run = vi.fn(() => true), input = vi.fn();
  const view = render(<Shortcuts run={run} />);
  const textarea = view.getByRole('textbox'); textarea.addEventListener('keydown', input);
  const down = key('KeyB'); fireEvent(textarea, down);
  expect(down.defaultPrevented).toBe(true); expect(input).not.toHaveBeenCalled();
  const repeat = key('KeyB', { repeat: true }); fireEvent(textarea, repeat);
  expect(repeat.defaultPrevented).toBe(true);
  const press = new KeyboardEvent('keypress', { code: 'KeyB', bubbles: true, cancelable: true }); fireEvent(textarea, press);
  expect(press.defaultPrevented).toBe(true);
  const up = new KeyboardEvent('keyup', { code: 'KeyB', bubbles: true, cancelable: true }); fireEvent(textarea, up);
  expect(up.defaultPrevented).toBe(true); expect(run).toHaveBeenCalledExactlyOnceWith('sessions');
  fireEvent(textarea, key('KeyB')); expect(run).toHaveBeenCalledTimes(2);
  view.unmount(); fireEvent(document, key('KeyB')); expect(run).toHaveBeenCalledTimes(2);
});

it('leaves unavailable actions and visible overlays alone, but ignores hidden overlays', () => {
  const run = vi.fn(() => false);
  const view = render(<><Shortcuts run={run} /><div role="dialog" hidden>Hidden confirmation</div></>);
  const unavailable = key('KeyB', { shiftKey: true }); fireEvent(document, unavailable);
  expect(run).toHaveBeenCalledWith('files'); expect(unavailable.defaultPrevented).toBe(false);
  run.mockReturnValue(true);
  const allowed = key('KeyB'); fireEvent(document, allowed); expect(allowed.defaultPrevented).toBe(true);
  fireEvent(document, new KeyboardEvent('keyup', { code: 'KeyB' }));
  view.rerender(<><Shortcuts run={run} /><div role="dialog">Visible confirmation</div></>);
  const blocked = key('KeyB'); fireEvent(document, blocked); expect(blocked.defaultPrevented).toBe(false);
  expect(run).toHaveBeenCalledTimes(2);
  view.rerender(<><Shortcuts run={run} /><div role="menu">Context menu</div></>);
  fireEvent(document, key('KeyB')); expect(run).toHaveBeenCalledTimes(2);
  view.rerender(<><Shortcuts run={run} /><div role="alertdialog">Unsaved file confirmation</div></>);
  const confirmation = key('KeyE', { shiftKey: true }); fireEvent(document, confirmation);
  expect(confirmation.defaultPrevented).toBe(false); expect(run).toHaveBeenCalledTimes(2);
});

it('clears held shortcuts on window blur and uses the latest handler', () => {
  const first = vi.fn(() => true), next = vi.fn(() => true);
  const view = render(<Shortcuts run={first} />);
  fireEvent(document, key('KeyB')); fireEvent(window, new Event('blur'));
  view.rerender(<Shortcuts run={next} />);
  fireEvent(document, key('KeyB')); expect(next).toHaveBeenCalledOnce();
});
