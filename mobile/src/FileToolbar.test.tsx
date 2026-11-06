// @vitest-environment jsdom
import React, { type ReactNode } from 'react';
import { cleanup, render, screen } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, expect, it, vi } from 'vitest';
import { FileToolbar } from './FileToolbar';
import { FileIconButton } from './FileIconButton';

function flatten(value: unknown): Record<string, unknown> {
  if (Array.isArray(value)) return value.reduce((result, item) => ({ ...result, ...flatten(item) }), {});
  return value && typeof value === 'object' ? Object.fromEntries(Object.entries(value)) : {};
}
interface NativeProps { children?: ReactNode; testID?: string; style?: unknown }
vi.mock('react-native', () => ({
  View: ({ children, testID, style }: NativeProps) => <div data-testid={testID} data-style={JSON.stringify(flatten(style))}>{children}</div>,
  Text: ({ children, testID, style }: NativeProps) => <span data-testid={testID} data-style={JSON.stringify(flatten(style))}>{children}</span>,
  ActivityIndicator: () => <span role="progressbar" />,
  Pressable: ({ children, testID, style, accessibilityLabel, accessibilityHint, accessibilityState, disabled, onPress }: Omit<NativeProps, 'style'> & {
    style: (state: { pressed: boolean }) => unknown; accessibilityLabel: string; accessibilityHint?: string;
    accessibilityState: { selected?: boolean; busy?: boolean }; disabled?: boolean; onPress: () => void;
  }) => <button data-testid={testID} data-style={JSON.stringify(flatten(style({ pressed: false })))} data-hint={accessibilityHint}
    aria-label={accessibilityLabel} aria-pressed={accessibilityState.selected} aria-busy={accessibilityState.busy} disabled={disabled} onClick={onPress}>{children}</button>,
}));
vi.mock('./ui', () => ({ useUi: () => ({ colors: { text: '#fff', accent: '#f29', surface: '#222' } }) }));
afterEach(cleanup);
const actions = { close: vi.fn(), toggleMode: vi.fn(), path: vi.fn(), copy: vi.fn(), download: vi.fn(), upload: vi.fn(), refresh: vi.fn(), more: vi.fn(), save: vi.fn() };
const common = { ...actions, title: 'file.ts', file: true, editable: true, editing: false, text: true, dirty: false, saving: false,
  blocked: false, canTransfer: true, canCopy: true, canSave: true };
function style(element: HTMLElement): Record<string, unknown> { return flatten(JSON.parse(element.dataset.style ?? '{}')); }
it.each([
  ['read', false, false], ['edit', true, true], ['dirty preview', false, true],
] as const)('keeps %s to one36dp row with six32dp actions and108dp filename budget at320', (_name, editing, dirty) => {
  render(<FileToolbar {...common} editing={editing} dirty={dirty} />);
  const toolbar = screen.getByTestId('files-toolbar'), controls = screen.getAllByRole('button');
  expect(style(toolbar)).toMatchObject({ height: 36, flexDirection: 'row', gap: 2 }); expect(controls).toHaveLength(6);
  for (const control of controls) expect(style(control)).toMatchObject({ width: 32, height: 32, alignItems: 'center', justifyContent: 'center', flexShrink: 0 });
  expect(320 - controls.length * 32 - controls.length * 2 - 8).toBe(108);
  expect(style(screen.getByTestId('files-title'))).toMatchObject({ flex: 1, minWidth: 0 });
  if (editing || dirty) { expect(controls.at(-1)).toHaveAccessibleName('Save'); expect(screen.queryByRole('button', { name: 'Download selected file' })).toBeNull(); }
  else expect(screen.queryByRole('button', { name: 'Save' })).toBeNull();
  expect(screen.queryByRole('button', { name: /Wrap/ })).toBeNull();
});
it('centers actual upload/download arrow geometry rather than positioning a font baseline', () => {
  const view = render(<FileIconButton icon="download" label="Download" testID="arrow" onPress={vi.fn()} />);
  for (const icon of ['download', 'upload'] as const) {
    view.rerender(<FileIconButton icon={icon} label={icon} testID="arrow" onPress={vi.fn()} />);
    const glyph = screen.getByTestId('arrow-glyph'); expect(style(glyph)).toMatchObject({ width: 20, height: 20 });
    expect(glyph.textContent).toBe('');
    for (const node of Array.from(glyph.children)) {
      if (!(node instanceof HTMLElement)) throw new Error('Invalid glyph node.');
      const box = style(node);
      if (typeof box.left !== 'number' || typeof box.width !== 'number') throw new Error('Arrow stroke must have explicit geometry.');
      expect(box.left + box.width / 2).toBe(10);
    }
  }
});
it('keeps busy Save in its icon slot and avoids duplicate accessibility hints', () => {
  render(<FileToolbar {...common} editing dirty saving blocked />);
  const save = screen.getByRole('button', { name: 'Saving' }); expect(save).toBeDisabled(); expect(save).toHaveAttribute('aria-busy', 'true');
  expect(screen.getByTestId('files-save-glyph')).toContainElement(screen.getByRole('progressbar'));
  expect(save).not.toHaveAttribute('data-hint');
});
