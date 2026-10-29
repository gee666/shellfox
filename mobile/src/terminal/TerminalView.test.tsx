// @vitest-environment jsdom
import React, { type ReactNode } from 'react';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { TerminalView, type TerminalViewProps } from './TerminalView';
import { emptyModifiers } from './protocol';
import type { TerminalBridge } from './bridge';

type BridgeOptions = ConstructorParameters<typeof TerminalBridge>[0];
const native = vi.hoisted(() => ({
  visible: false,
  listeners: new Map<string, (event: { endCoordinates: { screenY: number } }) => void>(),
  dismiss: vi.fn(), show: vi.fn(async () => {}), copy: vi.fn(), read: vi.fn(async () => ''), alert: vi.fn(),
  bridge: { start: vi.fn(), dispose: vi.fn(), focus: vi.fn(), setModifiers: vi.fn(), key: vi.fn(),
    receive: vi.fn(), paste: vi.fn(), select: vi.fn(), setFontSize: vi.fn(), setTheme: vi.fn(), takeControl: vi.fn(async () => {}) },
  options: null as BridgeOptions | null,
}));
type NativeProps = {
  children?: ReactNode;
  accessibilityLabel?: string;
  accessibilityRole?: string;
  accessibilityState?: { selected?: boolean };
  disabled?: boolean;
  onPress?: () => void;
};
vi.mock('react-native', () => ({
  Platform: { OS: 'android' },
  Alert: { alert: (...args: unknown[]) => native.alert(...args) },
  StyleSheet: { create: <T,>(styles: T) => styles },
  Keyboard: {
    isVisible: () => native.visible,
    metrics: () => native.visible ? { screenY: 450 } : undefined,
    dismiss: () => native.dismiss(),
    addListener: (type: string, listener: (event: { endCoordinates: { screenY: number } }) => void) => {
      native.listeners.set(type, listener);
      return { remove: () => native.listeners.delete(type) };
    },
  },
  View: ({ children, accessibilityLabel }: NativeProps) => <div aria-label={accessibilityLabel}>{children}</div>,
  Text: ({ children }: NativeProps) => <span>{children}</span>,
  ScrollView: ({ children }: NativeProps) => <div>{children}</div>,
  KeyboardAvoidingView: ({ children }: NativeProps) => <div>{children}</div>,
  Modal: ({ children, visible }: NativeProps & { visible: boolean }) => visible ? <div>{children}</div> : null,
  TextInput: ({ value, onChangeText, accessibilityLabel }: {
    value: string; onChangeText: (text: string) => void; accessibilityLabel: string;
  }) => <textarea aria-label={accessibilityLabel} value={value} onChange={event => onChangeText(event.target.value)} />,
  Pressable: ({ children, onPress, accessibilityLabel, accessibilityState, disabled }: NativeProps) =>
    <button aria-label={accessibilityLabel} aria-pressed={accessibilityState?.selected} disabled={disabled}
      onClick={onPress}>{children}</button>,
}));
vi.mock('react-native-safe-area-context', () => ({ useSafeAreaInsets: () => ({ bottom: 20 }) }));
vi.mock('react-native-webview', () => ({
  WebView: ({ onMessage }: { onMessage: (event: { nativeEvent: { data: string } }) => void }) =>
    <button aria-label="Page ready" onClick={() => onMessage({ nativeEvent: { data: JSON.stringify({ type: 'ready', viewId: 'view' }) } })} />,
}));
vi.mock('@react-native-clipboard/clipboard', () => ({ default: {
  setString: (text: string) => native.copy(text), getString: () => native.read(),
} }));
vi.mock('./native-keyboard', () => ({ showTerminalKeyboard: () => native.show() }));
vi.mock('./terminal-html', () => ({ TERMINAL_HTML: '' }));
vi.mock('./bridge', () => ({
  TerminalBridge: class {
    constructor(options: BridgeOptions) { native.options = options; return native.bridge; }
  },
}));
const props: TerminalViewProps = {
  api: { attachTerminal: vi.fn(), writeTerminal: vi.fn(), resizeTerminal: vi.fn(), acknowledgeTerminal: vi.fn(), detachTerminal: vi.fn(), takeTerminalControl: vi.fn() },
  subscribeTerminal: () => () => {}, tabId: 'tab', fontSize: 14, connectionEpoch: 'connection',
};
function keyboard(visible: boolean) {
  act(() => {
    native.visible = visible;
    native.listeners.get(visible ? 'keyboardDidShow' : 'keyboardDidHide')?.({ endCoordinates: { screenY: 450 } });
  });
}
function setup(extra: Partial<TerminalViewProps> = {}) {
  const result = render(<TerminalView {...props} {...extra} />);
  fireEvent.click(screen.getByRole('button', { name: 'Page ready' }));
  return result;
}
beforeEach(() => { vi.clearAllMocks(); native.visible = false; native.options = null; });
afterEach(cleanup);

describe('mobile keyboard controls', () => {
  it('keeps replay selection and copy but hides automation input controls', () => {
    setup({ readOnly: true });
    expect(native.options?.readOnly).toBe(true);
    act(() => native.options?.onState?.('closed'));
    for (const name of ['Keyboard', 'Special keys', 'Ctrl', 'Paste', 'Esc', 'Attach']) {
      expect(screen.queryByRole('button', { name, exact: true })).toBeNull();
    }
    fireEvent.click(screen.getByRole('button', { name: 'Select', exact: true }));
    expect(native.bridge.select).toHaveBeenCalledWith(true);
    act(() => native.options?.onSelection?.('completed output'));
    fireEvent.click(screen.getByRole('button', { name: 'Copy', exact: true }));
    expect(native.copy).toHaveBeenCalledWith('completed output');
    expect(native.bridge.key).not.toHaveBeenCalled(); expect(native.bridge.paste).not.toHaveBeenCalled();
  });
  it('previews clipboard text and routes Send through terminal paste, not key input', async () => {
    setup();
    const text = '  日本語 🦊\r\nsecond line\n';
    native.read.mockResolvedValueOnce(text);
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Paste', exact: true })); });
    expect(screen.getByRole('textbox', { name: 'Text to paste into terminal' })).toHaveValue(text.replace(/\r\n/g, '\n'));
    expect(native.bridge.paste).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Send', exact: true }));
    expect(native.bridge.paste).toHaveBeenCalledExactlyOnceWith(text);
    expect(native.bridge.key).not.toHaveBeenCalled();
    expect(screen.queryByRole('textbox')).toBeNull();
  });
  it('copies the exact selection directly and shows confirmation without a second dialog', () => {
    setup();
    expect(screen.queryByRole('button', { name: 'Copy' })).toBeNull();
    const selection = '日本語\n  indented output\n';
    act(() => native.options?.onSelection?.(selection));
    fireEvent.click(screen.getByRole('button', { name: 'Copy' }));
    expect(native.copy).toHaveBeenCalledExactlyOnceWith(selection);
    expect(screen.getByRole('button', { name: 'Copied' })).toBeInTheDocument();
    expect(screen.queryByText('Select text to copy')).toBeNull();
    expect(native.bridge.key).not.toHaveBeenCalled();
    expect(native.bridge.paste).not.toHaveBeenCalled();
    act(() => native.options?.onSelection?.('new selection'));
    expect(screen.getByRole('button', { name: 'Copy' })).toBeInTheDocument();
    act(() => native.options?.onSelection?.(''));
    expect(screen.queryByRole('button', { name: 'Copy' })).toBeNull();
  });
  it('reports clipboard failures without claiming the text was copied', () => {
    setup();
    act(() => native.options?.onSelection?.('output'));
    native.copy.mockImplementationOnce(() => { throw new Error('unavailable'); });
    fireEvent.click(screen.getByRole('button', { name: 'Copy' }));
    expect(native.alert).toHaveBeenCalledWith('Could not copy', 'The clipboard is unavailable. Try again.');
    expect(screen.queryByRole('button', { name: 'Copied' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Copy' }));
    expect(screen.getByRole('button', { name: 'Copied' })).toBeInTheDocument();
  });
  it('uses icon buttons and toggles mutually exclusive keyboards, including native back dismissal', () => {
    setup();
    const system = screen.getByRole('button', { name: 'Keyboard' });
    const special = screen.getByRole('button', { name: 'Special keys' });
    expect(system).not.toHaveTextContent('Keyboard');
    fireEvent.click(special);
    expect(screen.getByLabelText('Special keys keyboard')).toBeInTheDocument();
    expect(special).toHaveAttribute('aria-pressed', 'true');
    fireEvent.click(special);
    expect(screen.queryByLabelText('Special keys keyboard')).not.toBeInTheDocument();
    fireEvent.click(special);
    fireEvent.click(system);
    expect(screen.queryByLabelText('Special keys keyboard')).not.toBeInTheDocument();
    expect(native.bridge.focus).toHaveBeenLastCalledWith(true);
    keyboard(true);
    fireEvent.click(special);
    expect(native.bridge.focus).toHaveBeenLastCalledWith(false);
    // Wait for the native IME to disappear before allocating the custom panel.
    expect(screen.queryByLabelText('Special keys keyboard')).not.toBeInTheDocument();
    keyboard(false);
    expect(screen.getByLabelText('Special keys keyboard')).toBeInTheDocument();
    fireEvent.click(system);
    keyboard(true);
    keyboard(false); // Android back or native dismissal.
    expect(system).toHaveAttribute('aria-pressed', 'false');
  });
  it('retains sticky modifiers and sends named keys, not pre-encoded escape strings', () => {
    setup();
    fireEvent.click(screen.getByRole('button', { name: 'Ctrl' }));
    fireEvent.click(screen.getByRole('button', { name: 'Special keys' }));
    fireEvent.click(screen.getByRole('button', { name: 'j' }));
    expect(native.bridge.key).toHaveBeenLastCalledWith({ key: 'j', modifiers: { ...emptyModifiers, ctrl: true } });
    fireEvent.click(screen.getByRole('button', { name: 'Alt' }));
    fireEvent.click(screen.getByRole('button', { name: 'Shift' }));
    fireEvent.click(screen.getByRole('button', { name: 'Super' }));
    fireEvent.click(screen.getByRole('button', { name: 'CapsLock' }));
    const modifiers = { ctrl: true, alt: true, shift: true, meta: true, capsLock: true };
    expect(native.bridge.setModifiers).toHaveBeenLastCalledWith(modifiers);
    fireEvent.click(screen.getByRole('button', { name: 'F12' }));
    expect(native.bridge.key).toHaveBeenLastCalledWith({ key: 'F12', modifiers });
    fireEvent.click(screen.getByRole('button', { name: 'PgUp' }));
    expect(native.bridge.key).toHaveBeenLastCalledWith({ key: 'PageUp', modifiers });
    fireEvent.click(screen.getByRole('button', { name: 'Keyboard' }));
    fireEvent.click(screen.getByRole('button', { name: 'Esc' }));
    expect(native.bridge.key).toHaveBeenLastCalledWith({ key: 'Escape', modifiers });
    fireEvent.click(screen.getByRole('button', { name: 'Ctrl' }));
    expect(native.bridge.setModifiers).toHaveBeenLastCalledWith({ ...modifiers, ctrl: false });
    expect(emptyModifiers.ctrl).toBe(false);
  });
  it('hides terminal controls and offers explicit Take it while another client owns the view', async () => {
    setup(); fireEvent.click(screen.getByRole('button', { name: 'Special keys' }));
    act(() => native.options?.onSelection?.('private output'));
    expect(screen.getByRole('button', { name: 'Copy' })).toBeInTheDocument();
    act(() => native.options?.onState?.('blocked'));
    expect(screen.getByText('Another client is controlling this terminal')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Keyboard' })).toBeNull();
    expect(screen.queryByLabelText('Special keys keyboard')).toBeNull();
    expect(native.dismiss).toHaveBeenCalled();
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Take it' })); });
    expect(native.bridge.takeControl).toHaveBeenCalledOnce();
    act(() => native.options?.onState?.('attached'));
    expect(screen.queryByText('Another client is controlling this terminal')).toBeNull();
    expect(screen.getByRole('button', { name: 'Keyboard' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Copy' })).toBeNull();
    expect(native.bridge.start).toHaveBeenCalledOnce();
  });
  it('forwards current state/error callbacks without recreating the bridge', () => {
    const first = vi.fn(), next = vi.fn(), error = vi.fn();
    const result = setup({ onState: first });
    act(() => native.options?.onState?.('attaching'));
    expect(first).toHaveBeenCalledWith('attaching');
    result.rerender(<TerminalView {...props} onState={next} onError={error} />);
    act(() => {
      native.options?.onState?.('attached');
      native.options?.onError(new Error('test'));
    });
    expect(next).toHaveBeenCalledWith('attached');
    expect(error).toHaveBeenCalledWith(new Error('test'));
    expect(native.bridge.start).toHaveBeenCalledTimes(1);
    expect(native.bridge.dispose).not.toHaveBeenCalled();
  });
});
