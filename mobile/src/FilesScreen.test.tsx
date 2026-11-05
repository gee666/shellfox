// @vitest-environment jsdom
import React, { type ReactNode } from 'react';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { Alert } from 'react-native';
import { success, failure } from '../../src/shared/contracts';
import type { BrowserDirectory, BrowserFile, FileApi } from './file-browser-types';
import { FilesScreen } from './FilesScreen';
import { getImageDraftScope } from '../../src/shared/image-editor-drafts';
import Clipboard from '@react-native-clipboard/clipboard';
import { discardImageEditorDraft, type FileImageEditorProps } from './FileImageEditor';

vi.mock('@react-native-clipboard/clipboard', () => ({ default: { setString: vi.fn() } }));
const transferMock = vi.hoisted(() => ({ progress: null, running: false, message: null, error: null, supported: false,
  download: vi.fn(), upload: vi.fn(), cancel: vi.fn(), dismiss: vi.fn() }));
vi.mock('./useFileTransfers', () => ({ useFileTransfers: () => transferMock }));
type NativeProps = { children?: ReactNode; testID?: string; accessibilityLabel?: string; disabled?: boolean; onPress?: () => void };
let hardwareBack: () => void;
let latestImageScope: object | undefined;
vi.mock('react-native', () => ({
  Alert: { alert: vi.fn() }, Platform: { OS: 'android' },
  Text: ({ children }: NativeProps) => <span>{children}</span>,
  View: ({ children, testID }: NativeProps) => <div data-testid={testID}>{children}</div>,
  ScrollView: ({ children }: NativeProps) => <div>{children}</div>,
  KeyboardAvoidingView: ({ children }: NativeProps) => <div>{children}</div>,
  Modal: ({ children, onRequestClose }: NativeProps & { onRequestClose: () => void }) => { hardwareBack = onRequestClose; return <div role="dialog">{children}</div>; },
  Pressable: ({ children, onPress, accessibilityLabel, disabled }: NativeProps) => <button aria-label={accessibilityLabel} onClick={onPress} disabled={disabled}>{children}</button>,
  TextInput: ({ value, onChangeText, accessibilityLabel, editable, testID }: NativeProps & { value: string; onChangeText: (value: string) => void; editable?: boolean }) =>
    <textarea data-testid={testID} aria-label={accessibilityLabel} value={value} disabled={editable === false} onChange={event => onChangeText(event.target.value)} />,
  Image: ({ accessibilityLabel }: NativeProps) => <div role="img" aria-label={accessibilityLabel} />,
  FlatList: ({ data, renderItem, ListEmptyComponent }: { data: BrowserDirectory['entries']; renderItem: (value: { item: BrowserDirectory['entries'][number] }) => ReactNode; ListEmptyComponent: ReactNode }) =>
    <div>{data.length ? data.map(item => <div key={item.path}>{renderItem({ item })}</div>) : ListEmptyComponent}</div>,
}));
vi.mock('react-native-safe-area-context', () => ({ SafeAreaView: ({ children, testID }: NativeProps) => <div data-testid={testID}>{children}</div> }));
vi.mock('./ui', () => ({
  useUi: () => ({ colors: {}, styles: {} }),
  Button: ({ title, onPress, disabled }: { title: string; onPress: () => void; disabled?: boolean }) => <button disabled={disabled} onClick={onPress}>{title}</button>,
  Note: ({ children }: NativeProps) => <span>{children}</span>,
}));
vi.mock('./DirectoryPathInput', () => ({ DirectoryPathInput: () => null }));
vi.mock('./FilePreview', () => ({ FilePreview: ({ text, kind, preparedMarkdown }: { text: string; kind: string; preparedMarkdown?: string }) =>
  <div data-testid="preview" data-kind={kind} data-prepared-markdown={preparedMarkdown}>{text}</div> }));
vi.mock('./FileImageEditor', () => ({ discardImageEditorDraft: vi.fn(), FileImageEditor: (props: FileImageEditorProps) => { latestImageScope = props.draftScope; return <div>
  <button onClick={() => props.onDirtyChange?.(true)}>Rotate image</button>
  <button onClick={() => { void props.onSave('data:image/png;base64,Yg==').then(result => {
    if (result.ok) { props.onDirtyChange?.(false); props.onSaved?.(result.value, 'data:image/png;base64,Yg=='); }
  }); }}>Save image</button>
</div>; } }));
const revision = 'a'.repeat(64);
const textFile: BrowserFile = { kind: 'text', path: '/work/code.ts', mimeType: 'text/plain', size: 4, revision, text: 'code' };
const directory: BrowserDirectory = { path: '/work', parentPath: '/', truncated: false, entries: [
  { name: 'src', path: '/work/src', kind: 'directory' }, { name: 'code.ts', path: '/work/code.ts', kind: 'file' },
] };
function setup(openRequest?: { path: string; line?: number; column?: number }) {
  const api = {
    list: vi.fn<FileApi['list']>(async () => success(directory)),
    read: vi.fn<FileApi['read']>(async () => success(textFile)),
    write: vi.fn<FileApi['write']>(async () => success({ path: textFile.path, revision: 'b'.repeat(64), size: 5 })),
    readAsset: vi.fn<FileApi['readAsset']>(async () => success({ size: 1, mimeType: 'text/javascript', dataUrl: 'data:text/javascript;base64,YQ==' })),
    writeImage: vi.fn<FileApi['writeImage']>(),
  };
  const onClose = vi.fn();
  const props = { api, sessionId: 'session', tabId: 'tab', available: true, onClose, openRequest };
  const view = render(<FilesScreen {...props} />);
  return { api, onClose, view, props };
}
async function press(name: string) { await act(async () => { fireEvent.click(screen.getByRole('button', { name, exact: true })); }); }
beforeEach(() => { vi.clearAllMocks(); transferMock.supported = false; });
afterEach(cleanup);
it('browses with session/tab context and goes back through folders before closing', async () => {
  const f = setup();
  await screen.findByRole('button', { name: 'Open folder src' });
  expect(f.api.list).toHaveBeenCalledWith({ sessionId: 'session', tabId: 'tab', path: '.' });
  f.api.list.mockResolvedValueOnce(success({ ...directory, path: '/work/src', entries: [] }));
  await press('Open folder src');
  expect(screen.getByLabelText('File path')).toHaveValue('/work/src');
  await act(async () => hardwareBack());
  expect(f.api.list).toHaveBeenLastCalledWith({ sessionId: 'session', tabId: 'tab', path: '/work' });
  await act(async () => hardwareBack());
  expect(f.onClose).toHaveBeenCalledOnce();
});
it('edits code and saves with the exact revision, then uses the returned revision', async () => {
  const f = setup(); await screen.findByRole('button', { name: 'Open file code.ts' });
  await press('Open file code.ts');
  expect(screen.getByTestId('preview')).toHaveAttribute('data-kind', 'code');
  await press('Source');
  fireEvent.change(screen.getByLabelText('File source'), { target: { value: 'saved' } });
  await press('Save');
  expect(f.api.write).toHaveBeenCalledWith({ sessionId: 'session', tabId: 'tab', path: textFile.path, text: 'saved', revision });
  expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();
  fireEvent.change(screen.getByLabelText('File source'), { target: { value: 'again' } });
  await press('Save');
  expect(f.api.write).toHaveBeenLastCalledWith(expect.objectContaining({ revision: 'b'.repeat(64), text: 'again' }));
});
it('retains a conflicted draft and requires explicit discard for reload or modal back', async () => {
  const f = setup({ path: './code.ts', line: 1 }); await screen.findByLabelText('File source');
  f.api.write.mockResolvedValueOnce(failure('FILE_CONFLICT', 'Changed on host. Reload first.'));
  fireEvent.change(screen.getByLabelText('File source'), { target: { value: 'unsaved' } });
  await press('Save');
  expect(screen.getByText('Changed on host. Reload first.')).toBeInTheDocument();
  expect(screen.getByLabelText('File source')).toHaveValue('unsaved');
  await act(async () => hardwareBack());
  expect(Alert.alert).toHaveBeenCalledWith('Discard changes?', expect.any(String), expect.any(Array));
  expect(f.api.list).not.toHaveBeenCalled();
  await press('Reload');
  const buttons = vi.mocked(Alert.alert).mock.calls.at(-1)?.[2];
  await act(async () => buttons?.find(button => button.text === 'Discard')?.onPress?.());
  expect(screen.getByLabelText('File source')).toHaveValue('code');
});
it('closes explicitly without losing a dirty draft until discard is confirmed', async () => {
  const f = setup({ path: './code.ts', line: 1 }); await screen.findByLabelText('File source');
  fireEvent.change(screen.getByLabelText('File source'), { target: { value: 'unsaved' } });
  await press('‹ Terminal'); expect(f.onClose).not.toHaveBeenCalled();
  const buttons = vi.mocked(Alert.alert).mock.calls.at(-1)?.[2];
  await act(async () => buttons?.find(button => button.text === 'Discard')?.onPress?.());
  expect(f.onClose).toHaveBeenCalledOnce();
});
it('blocks duplicate writes and navigation until a pending save completes', async () => {
  const f = setup({ path: './code.ts', line: 1 }); await screen.findByLabelText('File source');
  let finish!: (value: Awaited<ReturnType<FileApi['write']>>) => void;
  f.api.write.mockReturnValueOnce(new Promise(resolve => { finish = resolve; }));
  fireEvent.change(screen.getByLabelText('File source'), { target: { value: 'unsaved' } });
  await press('Save');
  expect(screen.getByLabelText('File source')).toBeDisabled();
  await act(async () => hardwareBack()); expect(f.api.list).not.toHaveBeenCalled();
  await act(async () => finish(success({ path: textFile.path, revision: 'b'.repeat(64), size: 7 })));
  expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();
});
it('keeps the draft on disconnect and does not allow a save or navigation request', async () => {
  const f = setup({ path: './code.ts', line: 1 }); await screen.findByLabelText('File source');
  fireEvent.change(screen.getByLabelText('File source'), { target: { value: 'unsaved' } });
  f.view.rerender(<FilesScreen {...f.props} available={false} />);
  expect(screen.getByLabelText('File source')).toHaveValue('unsaved');
  expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();
  expect(screen.getByRole('button', { name: 'Go' })).toBeDisabled();
  expect(f.api.write).not.toHaveBeenCalled();
});
it('renders markdown first and keeps source edits when switching preview modes', async () => {
  const f = setup(); f.api.read.mockResolvedValueOnce(success({ ...textFile, path: '/work/readme.md', mimeType: 'text/markdown', text: '# Hello' }));
  await screen.findByRole('button', { name: 'Open file code.ts' }); await press('Open file code.ts');
  expect(screen.getByTestId('preview')).toHaveAttribute('data-kind', 'markdown');
  await press('Source'); fireEvent.change(screen.getByLabelText('File source'), { target: { value: '# Edited' } });
  await press('Preview'); expect(screen.getByTestId('preview')).toHaveTextContent('# Edited');
});
it('resolves Markdown sibling images using the Markdown file path and paired host context', async () => {
  const f = setup();
  f.api.read.mockResolvedValueOnce(success({ ...textFile, path: '/work/docs/README.md', mimeType: 'text/markdown', text: '# Diagram\n\n![Diagram](images/diagram.png)' }));
  f.api.readAsset.mockResolvedValueOnce(success({ size: 1, mimeType: 'image/png', dataUrl: 'data:image/png;base64,YQ==' }));
  await screen.findByRole('button', { name: 'Open file code.ts' }); await press('Open file code.ts');
  await waitFor(() => expect(f.api.readAsset).toHaveBeenCalledExactlyOnceWith({ sessionId: 'session', tabId: 'tab', path: '/work/docs/README.md', assetPath: 'images/diagram.png' }));
  await waitFor(() => expect(screen.getByTestId('preview').getAttribute('data-prepared-markdown')).toContain('src="data:image/png;base64,YQ=="'));
  expect(screen.getByTestId('preview').getAttribute('data-prepared-markdown')).toContain('<h1>Diagram</h1>');
  await press('Source');
  expect(screen.getByLabelText('File source')).toHaveValue('# Diagram\n\n![Diagram](images/diagram.png)');
});
it('keeps Markdown readable when sibling images are missing and does not fetch external URLs', async () => {
  const f = setup();
  f.api.read.mockResolvedValueOnce(success({ ...textFile, path: '/work/README.md', mimeType: 'text/markdown', text: '# Readable\n\n![Missing](missing.png) ![External](https://example.invalid/image.png)' }));
  f.api.readAsset.mockResolvedValueOnce(failure('NOT_FOUND', 'Image missing.'));
  await screen.findByRole('button', { name: 'Open file code.ts' }); await press('Open file code.ts');
  await waitFor(() => expect(screen.getByTestId('preview').getAttribute('data-prepared-markdown')).toContain('src="data:,"'));
  expect(f.api.readAsset).toHaveBeenCalledExactlyOnceWith({ sessionId: 'session', tabId: 'tab', path: '/work/README.md', assetPath: 'missing.png' });
  expect(screen.getByTestId('preview').getAttribute('data-prepared-markdown')).toContain('<h1>Readable</h1>');
});
it('resolves sibling HTML scripts through the scoped API, never a file URL', async () => {
  const f = setup(); f.api.read.mockResolvedValueOnce(success({ ...textFile, path: '/work/index.html', mimeType: 'text/html', text: '<script src="local.js"></script>' }));
  await screen.findByRole('button', { name: 'Open file code.ts' }); await press('Open file code.ts');
  await waitFor(() => expect(f.api.readAsset).toHaveBeenCalledWith({ sessionId: 'session', tabId: 'tab', path: '/work/index.html', assetPath: 'local.js' }));
  await waitFor(() => expect(screen.getByTestId('preview')).toHaveTextContent('data:text/javascript;base64,YQ=='));
});
it('previews SVG as a static image document while allowing source editing', async () => {
  const f = setup(); f.api.read.mockResolvedValueOnce(success({ ...textFile, path: '/work/icon.svg', mimeType: 'image/svg+xml', text: '<svg />' }));
  await screen.findByRole('button', { name: 'Open file code.ts' }); await press('Open file code.ts');
  expect(screen.getByTestId('preview')).toHaveAttribute('data-kind', 'svg');
  await press('Source'); expect(screen.getByLabelText('File source')).toHaveValue('<svg />');
});
it('saves image edits using their original revision and updates the next save', async () => {
  const f = setup();
  f.api.read.mockResolvedValueOnce(success({ kind: 'image', path: '/work/image.png', revision, size: 1, mimeType: 'image/png', dataUrl: 'data:image/png;base64,YQ==' }));
  f.api.writeImage.mockResolvedValue(success({ path: '/work/image.png', revision: 'b'.repeat(64), size: 1 }));
  await screen.findByRole('button', { name: 'Open file code.ts' }); await press('Open file code.ts');
  await press('Edit image');
  expect(latestImageScope).toBe(getImageDraftScope(f.api, 'session', 'tab'));
  await press('Rotate image'); await press('Save image');
  expect(f.api.writeImage).toHaveBeenLastCalledWith({ sessionId: 'session', tabId: 'tab', path: '/work/image.png', revision, dataUrl: 'data:image/png;base64,Yg==' });
  await press('Rotate image'); await press('Save image');
  expect(f.api.writeImage).toHaveBeenLastCalledWith(expect.objectContaining({ revision: 'b'.repeat(64) }));
});
it('guards dirty image navigation even after switching back to Preview, and clears the discarded operation draft', async () => {
  const f = setup();
  const image = { kind: 'image' as const, path: '/work/image.png', revision, size: 1, mimeType: 'image/png', dataUrl: 'data:image/png;base64,YQ==' };
  f.api.read.mockResolvedValueOnce(success(image));
  await screen.findByRole('button', { name: 'Open file code.ts' }); await press('Open file code.ts');
  await press('Edit image'); await press('Rotate image'); await press('Preview'); await press('‹ Terminal');
  expect(f.onClose).not.toHaveBeenCalled();
  const buttons = vi.mocked(Alert.alert).mock.calls.at(-1)?.[2];
  await act(async () => buttons?.find(button => button.text === 'Discard')?.onPress?.());
  expect(discardImageEditorDraft).toHaveBeenCalledExactlyOnceWith(image, getImageDraftScope(f.api, 'session', 'tab'));
  expect(f.onClose).toHaveBeenCalledOnce();
});
it('keeps unsupported images preview-only', async () => {
  const f = setup();
  f.api.read.mockResolvedValueOnce(success({ kind: 'image', path: '/work/image.gif', revision, size: 1, mimeType: 'image/gif', dataUrl: 'data:image/gif;base64,YQ==' }));
  await screen.findByRole('button', { name: 'Open file code.ts' }); await press('Open file code.ts');
  expect(screen.getByRole('img', { name: '/work/image.gif' })).toBeInTheDocument();
  expect(screen.queryByRole('button', { name: 'Edit image' })).toBeNull();
});
it.each(['image/x-icon', 'image/vnd.microsoft.icon', 'image/avif'])('uses a static WebView image for %s rather than platform-native decoding', async mimeType => {
  const f = setup();
  f.api.read.mockResolvedValueOnce(success({ kind: 'image', path: '/work/image.ico', revision, size: 1, mimeType, dataUrl: `data:${mimeType};base64,YQ==` }));
  await screen.findByRole('button', { name: 'Open file code.ts' }); await press('Open file code.ts');
  expect(screen.getByTestId('preview')).toHaveAttribute('data-kind', 'image');
  expect(screen.queryByRole('button', { name: 'Edit image' })).toBeNull();
});
it('keeps Download available for a selected file whose preview is too large', async () => {
  transferMock.supported = true;
  const f = setup(); await screen.findByRole('button', { name: 'Open file code.ts' });
  f.api.read.mockResolvedValueOnce(failure('UNSUPPORTED', 'Preview exceeds the size limit.'));
  f.api.list.mockResolvedValueOnce(failure('NOT_FOUND', 'Not a directory.'));
  await press('Open file code.ts');
  expect(screen.getByRole('button', { name: 'Download selected file' })).toBeEnabled();
  await press('Download selected file');
  expect(transferMock.download).toHaveBeenCalledExactlyOnceWith('/work/code.ts');
  await press('Upload to this folder'); expect(transferMock.upload).toHaveBeenCalledExactlyOnceWith('/work');
});
it('opens host home with no session context', async () => {
  const f = setup(); f.view.unmount(); f.api.list.mockClear();
  render(<FilesScreen api={f.api} available onClose={f.onClose} />);
  await screen.findByRole('button', { name: 'Open file code.ts' });
  expect(f.api.list).toHaveBeenCalledExactlyOnceWith({ path: '~' });
  expect(screen.getByRole('button', { name: 'Download selected file' })).toBeDisabled();
});
it('resets a guest filesystem context only after an explicit host-home submission', async () => {
  const f = setup(); await screen.findByRole('button', { name: 'Open file code.ts' });
  fireEvent.change(screen.getByLabelText('File path'), { target: { value: '~' } });
  f.api.list.mockResolvedValueOnce(success({ ...directory, path: 'C:/Users/host' }));
  await press('Go');
  expect(f.api.list).toHaveBeenLastCalledWith({ path: '~' });
  await press('Open file code.ts');
  expect(f.api.read).toHaveBeenLastCalledWith({ path: '/work/code.ts' });
});
it('copies the canonical opened host or guest file path to the local clipboard without line suffixes', async () => {
  setup({ path: './code.ts', line: 12, column: 3 }); await screen.findByLabelText('File source');
  await press('Copy file path');
  expect(Clipboard.setString).toHaveBeenCalledExactlyOnceWith('/work/code.ts');
  expect(screen.getByText('Path copied.')).toBeInTheDocument();
});
it('shows clipboard errors without changing the opened file or draft', async () => {
  setup({ path: './code.ts', line: 1 }); await screen.findByLabelText('File source');
  vi.mocked(Clipboard.setString).mockImplementationOnce(() => { throw new Error('Clipboard unavailable.'); });
  await press('Copy file path');
  expect(screen.getByText('Clipboard unavailable.')).toBeInTheDocument();
  expect(screen.getByLabelText('File source')).toHaveValue('code');
});
it('shows errors without replacing the last working directory', async () => {
  const f = setup(); await screen.findByRole('button', { name: 'Open file code.ts' });
  f.api.read.mockResolvedValueOnce(failure('NOT_FOUND', 'File disappeared.'));
  f.api.list.mockResolvedValueOnce(failure('NOT_FOUND', 'No directory.'));
  await press('Open file code.ts');
  expect(screen.getByText('File disappeared.')).toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'Open file code.ts' })).toBeInTheDocument();
});
