// @vitest-environment jsdom
import React, { type ReactNode } from 'react';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { TerminalKeys } from './TerminalKeys';

const native = vi.hoisted(() => ({ os: 'android', read: vi.fn<() => Promise<string>>(), alert: vi.fn() }));
type NativeProps = { children?: ReactNode; accessibilityLabel?: string; onPress?: () => void; disabled?: boolean };
vi.mock('react-native', () => ({
  Platform: { get OS() { return native.os; } },
  Alert: { alert: (...args: unknown[]) => native.alert(...args) },
  StyleSheet: { create: <T,>(styles: T) => styles },
  View: ({ children }: NativeProps) => <div>{children}</div>,
  Text: ({ children }: NativeProps) => <span>{children}</span>,
  ScrollView: ({ children }: NativeProps) => <div>{children}</div>,
  KeyboardAvoidingView: ({ children }: NativeProps) => <div>{children}</div>,
  Modal: ({ children, visible, onRequestClose }: NativeProps & { visible: boolean; onRequestClose: () => void }) =>
    visible ? <div role="dialog" onKeyDown={event => { if (event.key === 'Escape') onRequestClose(); }}>{children}</div> : null,
  TextInput: ({ value, onChangeText, accessibilityLabel }: {
    value: string; onChangeText: (text: string) => void; accessibilityLabel: string;
  }) => <textarea aria-label={accessibilityLabel} value={value} onChange={event => onChangeText(event.target.value)} />,
  Pressable: ({ children, onPress, accessibilityLabel, disabled }: NativeProps) =>
    <button aria-label={accessibilityLabel} disabled={disabled} onClick={onPress}>{children}</button>,
}));
vi.mock('@react-native-clipboard/clipboard', () => ({ default: { getString: () => native.read(), setString: vi.fn() } }));

function setup() {
  const onPaste = vi.fn();
  const result = render(<TerminalKeys bottomInset={0} control={false} selecting={false} selection=""
    onControl={vi.fn()} onSelect={vi.fn()} onKey={vi.fn()} systemKeyboard={false} specialKeyboard={false}
    onKeyboard={vi.fn()} onSpecialKeyboard={vi.fn()} onPaste={onPaste} />);
  return { ...result, onPaste };
}
function press(name: string) { fireEvent.click(screen.getByRole('button', { name, exact: true })); }
function editor() { return screen.getByRole('textbox', { name: 'Text to paste into terminal' }); }
function deferredClipboard() {
  let resolve!: (text: string) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<string>((res, rej) => { resolve = res; reject = rej; });
  native.read.mockReturnValueOnce(promise);
  return {
    resolve: async (text: string) => { await act(async () => { resolve(text); }); },
    reject: async () => { await act(async () => { reject(new Error('unavailable')); }); },
  };
}
beforeEach(() => { vi.resetAllMocks(); native.read.mockResolvedValue(''); });
afterEach(cleanup);

describe.each(['android', 'ios'])('mobile paste dialog on %s', os => {
  beforeEach(() => { native.os = os; });

  it('reads the local clipboard only on opening and sends the exact edited text only on Send', async () => {
    const { onPaste } = setup();
    expect(native.read).not.toHaveBeenCalled();
    const text = '  日本語 🦊\nsecond line\n';
    native.read.mockResolvedValueOnce(text);
    await act(async () => { press('Paste'); });
    expect(native.read).toHaveBeenCalledOnce();
    expect(editor()).toHaveValue(text);
    expect(screen.getByRole('button', { name: 'Send', exact: true })).toBeEnabled();
    expect(onPaste).not.toHaveBeenCalled();
    fireEvent.change(editor(), { target: { value: `${text}edited` } });
    press('Send');
    expect(onPaste).toHaveBeenCalledExactlyOnceWith(`${text}edited`);
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('reads fresh clipboard contents on every opening, including after Send', async () => {
    setup();
    native.read.mockResolvedValueOnce('first').mockResolvedValueOnce('second').mockResolvedValueOnce('third');
    await act(async () => { press('Paste'); });
    expect(editor()).toHaveValue('first');
    press('Cancel');
    await act(async () => { press('Paste'); });
    expect(editor()).toHaveValue('second');
    press('Send');
    await act(async () => { press('Paste'); });
    expect(editor()).toHaveValue('third');
    expect(native.read).toHaveBeenCalledTimes(3);
  });

  it('leaves an empty clipboard editable and disables Send until there is text', async () => {
    const { onPaste } = setup();
    await act(async () => { press('Paste'); });
    expect(editor()).toHaveValue('');
    expect(screen.getByRole('button', { name: 'Send', exact: true })).toBeDisabled();
    expect(native.alert).not.toHaveBeenCalled();
    fireEvent.change(editor(), { target: { value: 'manual paste' } });
    press('Send');
    expect(onPaste).toHaveBeenCalledExactlyOnceWith('manual paste');
  });

  it.each(['rejection', 'throw'])('keeps manual paste available after a clipboard %s', async failure => {
    const { onPaste } = setup();
    if (failure === 'rejection') native.read.mockRejectedValueOnce(new Error('denied'));
    else native.read.mockImplementationOnce(() => { throw new Error('unavailable'); });
    await act(async () => { press('Paste'); });
    expect(native.alert).toHaveBeenCalledExactlyOnceWith('Could not paste',
      'The clipboard is unavailable. Touch and hold in the text field to paste.');
    expect(editor()).toHaveValue('');
    fireEvent.change(editor(), { target: { value: 'fallback' } });
    press('Send');
    expect(onPaste).toHaveBeenCalledExactlyOnceWith('fallback');
  });

  it.each(['resolve', 'reject'] as const)('ignores a late clipboard %s after manual edits', async outcome => {
    const { onPaste } = setup();
    const pending = deferredClipboard();
    press('Paste');
    fireEvent.change(editor(), { target: { value: 'keep my edit' } });
    await pending[outcome]('old clipboard');
    expect(editor()).toHaveValue('keep my edit');
    expect(native.alert).not.toHaveBeenCalled();
    press('Send');
    expect(onPaste).toHaveBeenCalledExactlyOnceWith('keep my edit');
  });

  it.each(['Cancel', 'native close'])('ignores an old read after %s and reopening', async close => {
    const { onPaste } = setup();
    const old = deferredClipboard();
    press('Paste');
    if (close === 'Cancel') press('Cancel');
    else fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(onPaste).not.toHaveBeenCalled();
    const current = deferredClipboard();
    press('Paste');
    await current.resolve('new clipboard');
    await old.resolve('stale clipboard');
    expect(editor()).toHaveValue('new clipboard');
    expect(native.read).toHaveBeenCalledTimes(2);
    expect(onPaste).not.toHaveBeenCalled();
  });

  it('clears the previous draft while a fresh read is pending', async () => {
    setup();
    native.read.mockResolvedValueOnce('old draft');
    await act(async () => { press('Paste'); });
    press('Cancel');
    const pending = deferredClipboard();
    press('Paste');
    expect(editor()).toHaveValue('');
    expect(screen.getByRole('button', { name: 'Send', exact: true })).toBeDisabled();
    await pending.resolve('fresh draft');
    expect(editor()).toHaveValue('fresh draft');
  });

  it.each(['cancel', 'unmount'])('suppresses a late clipboard failure after %s', async close => {
    const { unmount, onPaste } = setup();
    const pending = deferredClipboard();
    press('Paste');
    if (close === 'cancel') press('Cancel');
    else unmount();
    await pending.reject();
    expect(native.alert).not.toHaveBeenCalled();
    expect(onPaste).not.toHaveBeenCalled();
  });
});
