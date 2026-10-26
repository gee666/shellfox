// @vitest-environment jsdom
import React from 'react';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('react-native', () => ({
  StyleSheet: { create: <T,>(styles: T) => styles },
  View: ({ children }: { children?: React.ReactNode }) => <div>{children}</div>,
  Text: ({ children }: { children?: React.ReactNode }) => <span>{children}</span>,
  Modal: ({ children }: { children?: React.ReactNode }) => <section>{children}</section>,
  Pressable: ({ children, accessibilityLabel, accessibilityState, disabled, onPress }: {
    children?: React.ReactNode;
    accessibilityLabel?: string;
    accessibilityState?: { selected?: boolean; disabled?: boolean };
    disabled?: boolean;
    onPress?: () => void;
  }) => <button aria-label={accessibilityLabel} aria-pressed={accessibilityState?.selected} disabled={disabled} onClick={onPress}>{children}</button>,
  TextInput: ({ accessibilityLabel, value, onChangeText }: {
    accessibilityLabel?: string;
    value?: string;
    onChangeText?: (text: string) => void;
  }) => <input aria-label={accessibilityLabel} value={value} onChange={event => onChangeText?.(event.target.value)} />,
  ActivityIndicator: () => <div />,
}));

import { ColorSwatches } from './ColorSwatches';
import { ThemeProvider } from './theme';
import { useUi } from './ui';

afterEach(cleanup);

describe('mobile color controls', () => {
  it('selects presets and separately saves a custom choice', () => {
    const onChange = vi.fn(), onSave = vi.fn();
    const view = render(<ColorSwatches kind="accent" value="#ec4899" saved={[]} onChange={onChange} onSave={onSave} />);
    expect(screen.getByRole('button', { name: 'Use #ec4899' }).getAttribute('aria-pressed')).toBe('true');
    expect(screen.getByRole('button', { name: 'Save custom accent color' }).hasAttribute('disabled')).toBe(true);
    fireEvent.click(screen.getByRole('button', { name: 'Use #60a5fa' }));
    expect(onChange).toHaveBeenCalledWith('#60a5fa');
    expect(onSave).not.toHaveBeenCalled();
    view.rerender(<ColorSwatches kind="accent" value="#123456" saved={['#234567', '#345678', '#456789']} onChange={onChange} onSave={onSave} />);
    fireEvent.click(screen.getByRole('button', { name: 'Save custom accent color' }));
    expect(onSave).toHaveBeenCalledWith(['#345678', '#456789', '#123456']);
    fireEvent.click(screen.getByRole('button', { name: 'Use custom accent #234567' }));
    expect(onChange).toHaveBeenLastCalledWith('#234567');
  });
  it('applies a validated picker value, without automatically saving a swatch', () => {
    const onChange = vi.fn(), onSave = vi.fn();
    render(<ColorSwatches kind="background" value="#111016" saved={[]} onChange={onChange} onSave={onSave} />);
    fireEvent.click(screen.getByRole('button', { name: 'Background color', exact: true }));
    fireEvent.change(screen.getByLabelText('Hex color'), { target: { value: '#fff' } });
    expect(screen.getByRole('button', { name: 'Apply' }).hasAttribute('disabled')).toBe(true);
    fireEvent.change(screen.getByLabelText('Hex color'), { target: { value: '#ABCDEF' } });
    fireEvent.click(screen.getByRole('button', { name: 'Apply' }));
    expect(onChange).toHaveBeenCalledWith('#abcdef');
    expect(onSave).not.toHaveBeenCalled();
    expect(screen.queryByLabelText('Hex color')).toBeNull();
  });
  it('cancels picker edits and blocks duplicate saves', () => {
    const onChange = vi.fn();
    render(<ColorSwatches kind="accent" value="#123456" saved={['#123456']} onChange={onChange} onSave={vi.fn()} />);
    expect(screen.getByRole('button', { name: 'Save custom accent color' }).hasAttribute('disabled')).toBe(true);
    fireEvent.click(screen.getByRole('button', { name: 'Accent color', exact: true }));
    fireEvent.change(screen.getByLabelText('Hex color'), { target: { value: '#654321' } });
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onChange).not.toHaveBeenCalled();
  });
  it('rebuilds shared styles when theme inputs change', () => {
    function Consumer() {
      const { styles, colors } = useUi();
      return <output>{JSON.stringify({ bg: styles.page.backgroundColor, text: styles.label.color, buttonText: styles.buttonText.color, colors })}</output>;
    }
    const view = render(<ThemeProvider><Consumer /></ThemeProvider>);
    expect(screen.getByRole('status').textContent).toContain('"bg":"#111016"');
    view.rerender(<ThemeProvider backgroundColor="#ffffff" accentColor="#0000ff"><Consumer /></ThemeProvider>);
    const result: unknown = JSON.parse(screen.getByRole('status').textContent ?? 'null');
    expect(result).toMatchObject({ bg: '#ffffff', buttonText: '#ffffff', colors: { bg: '#ffffff', accent: '#0000ff' } });
    expect(screen.getByRole('status').textContent).not.toContain('"bg":"#111016"');
  });
});
