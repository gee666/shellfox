// @vitest-environment jsdom
import React, { useEffect, useState, type ReactNode } from 'react';
import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import type { TextInputProps } from 'react-native';
import { success } from '../../src/shared/contracts';
import type { BrowserFile } from './file-browser-types';
import { FileContent } from './FileContent';

interface NativeProps { children?: ReactNode; accessibilityLabel?: string; disabled?: boolean; onPress?: () => void; testID?: string }
interface ScrollProps extends NativeProps { horizontal?: boolean; scrollEnabled?: boolean; onLayout?: (event: { nativeEvent: { layout: { width: number } } }) => void }
let inputProps: TextInputProps | undefined;
let previewProps: { text: string; wrapLines?: boolean } | undefined;
let scrollProps: ScrollProps | undefined;
vi.mock('react-native', () => ({
  Platform: { OS: 'android' },
  View: ({ children }: NativeProps) => <div>{children}</div>,
  Text: ({ children }: NativeProps) => <span>{children}</span>,
  KeyboardAvoidingView: ({ children }: NativeProps) => <div>{children}</div>,
  Image: ({ accessibilityLabel }: NativeProps) => <div role="img" aria-label={accessibilityLabel} />,
  Pressable: ({ children, accessibilityLabel, disabled, onPress }: NativeProps) => <button aria-label={accessibilityLabel} disabled={disabled} onClick={onPress}>{children}</button>,
  ScrollView: (props: ScrollProps) => {
    scrollProps = props; useEffect(() => { props.onLayout?.({ nativeEvent: { layout: { width: 320 } } }); }, [props.onLayout]);
    return <div>{props.children}</div>;
  },
  TextInput: (props: TextInputProps) => {
    inputProps = props;
    return <textarea aria-label={props.accessibilityLabel} value={props.value} onChange={event => props.onChangeText?.(event.target.value)} />;
  },
}));
vi.mock('./ui', () => ({ useUi: () => ({ colors: {}, styles: {} }),
  Button: ({ title, onPress, disabled }: { title: string; onPress: () => void; disabled?: boolean }) => <button disabled={disabled} onClick={onPress}>{title}</button>,
  Note: ({ children }: NativeProps) => <span>{children}</span>,
}));
vi.mock('./FilePreview', () => ({ FilePreview: (props: { text: string; wrapLines?: boolean }) => { previewProps = props; return <div data-testid="code-preview">{props.text}</div>; } }));
vi.mock('./FileImageEditor', () => ({ FileImageEditor: () => <div data-testid="image-editor" /> }));
afterEach(() => { cleanup(); inputProps = undefined; previewProps = undefined; scrollProps = undefined; });
const text = 'const message = "' + 'wide '.repeat(100) + '";';
function setup(path = '/file.txt', source = text, image = false) {
  const file: BrowserFile = image ? { kind: 'image', path, mimeType: 'image/png', size: 3, revision: 'a'.repeat(64), dataUrl: 'data:image/png;base64,AAAA' }
    : { kind: 'text', path, mimeType: 'text/plain', size: source.length, revision: 'a'.repeat(64), text: source };
  function Content() {
    const [draft, setDraft] = useState(source);
    return <FileContent file={file} draft={draft} setDraft={setDraft} saving={false} available save={() => undefined} reload={() => undefined}
      imageEditor={{ onSave: async () => success({ path, size: 3, revision: 'b'.repeat(64) }) }} previewLoading={false} onPreviewError={() => undefined} />;
  }
  return render(<Content />);
}
it('wraps native source by default and offers horizontal scrolling without losing edits or remounting the input', () => {
  setup(); const input = screen.getByRole('textbox', { name: 'File source' });
  expect(scrollProps).toMatchObject({ horizontal: true, scrollEnabled: false }); expect(inputProps?.style).toMatchObject({ width: 320 });
  fireEvent.change(input, { target: { value: text + '\n// unsaved' } });
  fireEvent.click(screen.getByRole('button', { name: 'Unwrap' }));
  expect(scrollProps?.scrollEnabled).toBe(true); expect(inputProps?.style).toMatchObject({ width: expect.any(Number) });
  expect(screen.getByRole('textbox', { name: 'File source' })).toBe(input);
  expect(input).toHaveValue(text + '\n// unsaved'); expect(screen.getByText('Unsaved')).toBeVisible();
  fireEvent.click(screen.getByRole('button', { name: 'Wrap' }));
  expect(inputProps?.style).toMatchObject({ width: 320 }); expect(input).toHaveValue(text + '\n// unsaved');
});
it('keeps wrap preference and edited source when switching between native Source and highlighted Code', () => {
  setup('/file.ts'); expect(previewProps?.wrapLines).toBe(true);
  fireEvent.click(screen.getByRole('button', { name: 'Unwrap' })); expect(previewProps?.wrapLines).toBe(false);
  fireEvent.click(screen.getByRole('button', { name: 'Source' }));
  fireEvent.change(screen.getByRole('textbox', { name: 'File source' }), { target: { value: 'const dirty = 42;' } });
  fireEvent.click(screen.getByRole('button', { name: 'Code' }));
  expect(previewProps).toMatchObject({ wrapLines: false, text: 'const dirty = 42;' });
  fireEvent.click(screen.getByRole('button', { name: 'Wrap' })); expect(previewProps).toMatchObject({ wrapLines: true, text: 'const dirty = 42;' });
});
it('uses labeled icon-only image preview/edit/reload controls with no stale inline save limit', () => {
  setup('/image.png', '', true);
  for (const name of ['Preview', 'Edit image', 'Reload']) expect(screen.getByRole('button', { name })).toHaveTextContent('');
  fireEvent.click(screen.getByRole('button', { name: 'Edit image' }));
  expect(screen.getByTestId('image-editor')).toBeVisible(); expect(screen.queryByText(/512 KiB/)).not.toBeInTheDocument();
});
