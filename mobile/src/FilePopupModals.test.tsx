// @vitest-environment jsdom
import React, { type ReactNode } from 'react';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { FileMoreMenu } from './FileMoreMenu';
import { FilePathSheet } from './FilePathSheet';

interface ModalRecord { children?: ReactNode; supportedOrientations?: string[]; statusBarTranslucent?: boolean; navigationBarTranslucent?: boolean }
const records = vi.hoisted(() => ({ modal: vi.fn<(props: ModalRecord) => void>(), input: vi.fn<(props: { disableFullscreenUI?: boolean }) => void>() }));
interface NativeProps { children?: ReactNode; testID?: string; accessibilityLabel?: string; onPress?: () => void }
vi.mock('react-native', () => ({
  Keyboard: { dismiss: vi.fn() }, useWindowDimensions: () => ({ width: 1024, height: 768 }),
  Modal: (props: ModalRecord) => { records.modal(props); return <div data-testid="native-modal">{props.children}</div>; },
  View: ({ children, testID }: NativeProps) => <div data-testid={testID}>{children}</div>,
  Text: ({ children }: NativeProps) => <span>{children}</span>, ScrollView: ({ children }: NativeProps) => <div>{children}</div>,
  Pressable: ({ children, accessibilityLabel, onPress }: NativeProps) => <button aria-label={accessibilityLabel} onClick={onPress}>{children}</button>,
  TextInput: (props: { disableFullscreenUI?: boolean }) => { records.input(props); return <input />; },
}));
vi.mock('react-native-safe-area-context', () => ({
  SafeAreaProvider: ({ children }: NativeProps) => <div data-testid="fresh-safe-area">{children}</div>,
  useSafeAreaInsets: () => ({ top: 24, bottom: 24, left: 0, right: 0 }),
}));
vi.mock('./DirectoryPathInput', () => ({ DirectoryPathInput: () => null }));
vi.mock('./ui', () => ({ useUi: () => ({ colors: {} }), Note: ({ children }: NativeProps) => <span>{children}</span> }));
afterEach(() => { cleanup(); vi.clearAllMocks(); });
it.each(['path', 'more'])('the %s popup uses its own safe area and allows the iPad orientations already declared by the app', which => {
  if (which === 'path') render(<FilePathSheet value="/host/path" change={vi.fn()} submit={vi.fn()} close={vi.fn()} editable />);
  else render(<FileMoreMenu path="/host/path" items={[]} close={vi.fn()} />);
  expect(records.modal.mock.calls.at(-1)?.[0]).toMatchObject({ supportedOrientations: ['portrait', 'portrait-upside-down', 'landscape-left', 'landscape-right'], statusBarTranslucent: true, navigationBarTranslucent: true });
  expect(screen.getByTestId('native-modal').contains(screen.getByTestId('fresh-safe-area'))).toBe(true);
});
it('also disables Android fullscreen IME extraction in the legacy path fallback', () => {
  render(<FilePathSheet value="/host/path" change={vi.fn()} submit={vi.fn()} close={vi.fn()} editable />);
  expect(records.input).toHaveBeenCalledWith(expect.objectContaining({ disableFullscreenUI: true }));
});
