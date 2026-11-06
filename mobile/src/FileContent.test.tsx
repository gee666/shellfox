// @vitest-environment jsdom
import React, { useEffect, type ReactNode } from 'react';
import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import type { TextInputProps } from 'react-native';
import { success } from '../../src/shared/contracts';
import type { BrowserFile } from './file-browser-types';
import { FileContent } from './FileContent';

interface Range { start: number; end: number }
interface NativeProps { children?: ReactNode; testID?: string; accessibilityLabel?: string }
interface ScrollProps extends NativeProps { horizontal?: boolean; scrollEnabled?: boolean; onLayout?: (event: { nativeEvent: { layout: { width: number } } }) => void }
const native = vi.hoisted(() => ({
  setNativeProps: vi.fn<(value: { selection: Range }) => void>(), focus: vi.fn(),
  sync: vi.fn<(value: { text?: string; selection: Range }) => void>(), position: { start: -1, end: -1 }, emit: (_range: Range) => {},
}));
type NativeInputProps = Omit<TextInputProps, 'onSelectionChange' | 'onPressIn'> & {
  ref?: React.Ref<{ setNativeProps: typeof native.setNativeProps; focus: typeof native.focus }>;
  onSelectionChange?: (event: { nativeEvent: { selection: Range } }) => void;
  onPressIn?: () => void;
};
let inputProps: NativeInputProps | undefined;
let previewProps: { text: string; wrapLines?: boolean; kind: string } | undefined;
let scrollProps: ScrollProps | undefined;
vi.mock('react-native', () => ({
  Platform: { OS: 'android' },
  View: ({ children, testID }: NativeProps) => <div data-testid={testID}>{children}</div>,
  KeyboardAvoidingView: ({ children, testID }: NativeProps) => <div data-testid={testID}>{children}</div>,
  Image: ({ accessibilityLabel }: NativeProps) => <div role="img" aria-label={accessibilityLabel} />,
  ScrollView: (props: ScrollProps) => {
    scrollProps = props; useEffect(() => { props.onLayout?.({ nativeEvent: { layout: { width: 320 } } }); }, [props.onLayout]);
    return <div>{props.children}</div>;
  },
  TextInput: (props: NativeInputProps) => {
    inputProps = props;
    const [position, setPosition] = React.useState<Range>(() => ({ start: props.value?.length ?? 0, end: props.value?.length ?? 0 }));
    native.position = position;
    native.emit = range => { props.onSelectionChange?.({ nativeEvent: { selection: range } }); setPosition(range); };
    React.useImperativeHandle(props.ref, () => ({ focus: native.focus, setNativeProps: value => { native.setNativeProps(value); setPosition(value.selection); } }));
    // Model RN0.88's layout effect: native text/selection events update its own
    // state even if the caller ignores them, then controlled selection wins.
    React.useLayoutEffect(() => {
      if (!props.selection) return;
      const range = { start: props.selection.start, end: props.selection.end ?? props.selection.start };
      if (position.start === range.start && position.end === range.end) return;
      native.sync({ text: props.value, selection: range }); setPosition(range);
    }, [props.value, props.selection, position]);
    return <textarea aria-label={props.accessibilityLabel} value={props.value} onChange={event => props.onChangeText?.(event.target.value)} />;
  },
}));
vi.mock('./ui', () => ({ useUi: () => ({ colors: {}, styles: { grow: { flex: 1 } } }), Note: ({ children }: NativeProps) => <span>{children}</span> }));
vi.mock('./FilePreview', () => ({ FilePreview: (props: { text: string; wrapLines?: boolean; kind: string }) => { previewProps = props; return <div data-testid="preview">{props.text}</div>; } }));
vi.mock('./FileImageEditor', () => ({ FileImageEditor: () => <div data-testid="image-editor" /> }));
afterEach(() => { cleanup(); vi.useRealTimers(); native.setNativeProps.mockClear(); native.focus.mockReset(); native.sync.mockClear(); inputProps = undefined; previewProps = undefined; scrollProps = undefined; });
const source = 'const message = "' + 'wide '.repeat(100) + '";';
const file: BrowserFile = { kind: 'text', path: '/file.ts', mimeType: 'text/plain', size: source.length, revision: 'a'.repeat(64), text: source };
const common = { file, draft: source, setDraft: vi.fn(), saving: false, imageEditor: { onSave: async () => success({ path: file.path, size: 3, revision: 'b'.repeat(64) }) }, previewLoading: false, onPreviewError: vi.fn() };
const emit = (start: number, end = start) => act(() => native.emit({ start, end }));
it('is content-only with no navigation, metadata or action rows', () => {
  render(<FileContent {...common} mode="read" wrapLines />);
  expect(screen.getByTestId('files-content')).toBeInTheDocument(); expect(screen.queryAllByRole('button')).toHaveLength(0);
  expect(previewProps).toMatchObject({ kind: 'code', text: source, wrapLines: true }); expect(screen.queryByText(/bytes|Unsaved/)).toBeNull();
});
it('changes wrapping without remounting the native text input or losing selection/content', () => {
  const view = render(<FileContent {...common} mode="edit" wrapLines />); const input = screen.getByRole('textbox', { name: 'File source' });
  expect(scrollProps).toMatchObject({ horizontal: true, scrollEnabled: false }); expect(inputProps?.style).toMatchObject({ width: 320 });
  fireEvent.change(input, { target: { value: source + '\n// changed' } }); expect(common.setDraft).toHaveBeenLastCalledWith(source + '\n// changed');
  view.rerender(<FileContent {...common} draft={source + '\n// changed'} mode="edit" wrapLines={false} />);
  expect(scrollProps?.scrollEnabled).toBe(true); expect(inputProps?.style).toMatchObject({ width: expect.any(Number) });
  expect(screen.getByRole('textbox', { name: 'File source' })).toBe(input); expect(input).toHaveValue(source + '\n// changed');
});
it('focuses on the first explicit Edit transition without a saved caret and prevents fullscreen IME extraction', () => {
  const view = render(<FileContent {...common} mode="read" wrapLines />); expect(native.focus).not.toHaveBeenCalled();
  view.rerender(<FileContent {...common} mode="edit" wrapLines />);
  expect(native.focus).toHaveBeenCalledOnce(); expect(inputProps?.disableFullscreenUI).toBe(true);
});
it('captures and clamps the range before focus bootstrap events, then synchronizes it with the text', () => {
  const view = render(<FileContent {...common} mode="edit" wrapLines />); emit(20, 30);
  view.rerender(<FileContent {...common} mode="read" wrapLines />);
  native.focus.mockImplementationOnce(() => { native.emit({ start: 0, end: 0 }); native.emit({ start: 2, end: 2 }); });
  view.rerender(<FileContent {...common} draft="tiny" mode="edit" wrapLines />);
  expect(inputProps?.selection).toEqual({ start: 4, end: 4 }); expect(native.position).toEqual({ start: 4, end: 4 });
  expect(native.sync).toHaveBeenLastCalledWith({ text: 'tiny', selection: { start: 4, end: 4 } }); expect(native.setNativeProps).not.toHaveBeenCalled();
});
it('survives late initial text sync to END and releases control only after the matching native ACK', () => {
  const props = { ...common, draft: 'NativeSaved' };
  const view = render(<FileContent {...props} mode="edit" wrapLines />); emit(2);
  view.rerender(<FileContent {...props} mode="read" wrapLines />); view.rerender(<FileContent {...props} mode="edit" wrapLines />);
  emit(0); emit(props.draft.length); // The stale mount/text-sync events that overwrote the imperative setter.
  expect(inputProps?.selection).toEqual({ start: 2, end: 2 }); expect(native.position).toEqual({ start: 2, end: 2 });
  emit(2); expect(inputProps?.selection).toBeUndefined();
  emit(3); expect(native.position).toEqual({ start: 3, end: 3 });
  view.rerender(<FileContent {...props} mode="read" wrapLines />); view.rerender(<FileContent {...props} mode="edit" wrapLines />);
  expect(inputProps?.selection).toEqual({ start: 3, end: 3 });
});
it('never pins selection after genuine user press/text changes or the bounded fallback', async () => {
  vi.useFakeTimers(); const view = render(<FileContent {...common} mode="edit" wrapLines />); emit(8, 14);
  view.rerender(<FileContent {...common} mode="read" wrapLines />); view.rerender(<FileContent {...common} mode="edit" wrapLines />);
  act(() => inputProps?.onPressIn?.()); expect(inputProps?.selection).toBeUndefined(); emit(0);
  view.rerender(<FileContent {...common} mode="read" wrapLines />); view.rerender(<FileContent {...common} mode="edit" wrapLines />);
  expect(inputProps?.selection).toEqual({ start: 0, end: 0 });
  fireEvent.change(screen.getByRole('textbox'), { target: { value: 'typed' } }); expect(inputProps?.selection).toBeUndefined();
  emit(3); view.rerender(<FileContent {...common} mode="read" wrapLines />); view.rerender(<FileContent {...common} mode="edit" wrapLines />);
  await act(async () => { await vi.advanceTimersByTimeAsync(301); }); expect(inputProps?.selection).toBeUndefined();
});
it('applies a terminal line request only once without initial autofocus or pinned selection', async () => {
  vi.useFakeTimers(); const lineFile = { ...file, text: 'one\nsecond', size: 10 };
  const view = render(<FileContent {...common} file={lineFile} draft={lineFile.text} mode="edit" wrapLines openRequest={{ path: file.path, line: 2, column: 3 }} />);
  expect(inputProps?.selection).toEqual({ start: 6, end: 6 }); expect(native.focus).not.toHaveBeenCalled(); emit(0); expect(native.position).toEqual({ start: 6, end: 6 });
  await act(async () => { await vi.advanceTimersByTimeAsync(301); });
  view.rerender(<FileContent {...common} file={lineFile} draft={lineFile.text} mode="edit" wrapLines openRequest={{ path: file.path, line: 1, column: 1 }} />);
  expect(inputProps?.selection).toBeUndefined(); expect(native.focus).not.toHaveBeenCalled();
});
it('renders unsaved Markdown/HTML source in read mode, not saved contents', () => {
  render(<FileContent {...common} file={{ ...file, path: '/readme.md', mimeType: 'text/markdown' }} draft="# Unsaved" mode="read" wrapLines />);
  expect(previewProps).toMatchObject({ kind: 'markdown', text: '# Unsaved' });
});
it('delegates image editing without adding a native toolbar or bytes footer', () => {
  const image: BrowserFile = { kind: 'image', path: '/image.png', mimeType: 'image/png', size: 3, revision: 'a'.repeat(64), dataUrl: 'data:image/png;base64,AAAA' };
  render(<FileContent {...common} file={image} mode="edit" wrapLines />);
  expect(screen.getByTestId('image-editor')).toBeInTheDocument(); expect(screen.queryAllByRole('button')).toHaveLength(0);
});
