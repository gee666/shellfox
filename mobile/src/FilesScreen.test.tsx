// @vitest-environment jsdom
import React, { useEffect, type ReactNode } from 'react';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { Alert, Keyboard } from 'react-native';
import Clipboard from '@react-native-clipboard/clipboard';
import { success, failure } from '../../src/shared/contracts';
import { discardImageEditorDraft, getImageDraftScope } from '../../src/shared/image-editor-drafts';
import type { BrowserDirectory, BrowserFile, FileApi } from './file-browser-types';
import type { FileImageEditorProps } from './FileImageEditor';
import { FilesScreen } from './FilesScreen';
import type { FileMenuItem } from './FileMoreMenu';

const transfer = vi.hoisted(() => ({ progress: null, running: false, message: null, error: null, supported: false,
  download: vi.fn(), upload: vi.fn(), cancel: vi.fn(), dismiss: vi.fn() }));
const scroll = vi.hoisted(() => vi.fn());
vi.mock('./useFileTransfers', () => ({ useFileTransfers: () => transfer }));
vi.mock('@react-native-clipboard/clipboard', () => ({ default: { setString: vi.fn() } }));
vi.mock('../../src/shared/image-editor-drafts', async original => ({ ...await original<typeof import('../../src/shared/image-editor-drafts')>(), discardImageEditorDraft: vi.fn() }));
type NativeProps = { children?: ReactNode; testID?: string; accessibilityLabel?: string; disabled?: boolean; onPress?: () => void };
type ListProps = { data: BrowserDirectory['entries']; renderItem: (value: { item: BrowserDirectory['entries'][number] }) => ReactNode; ListEmptyComponent: ReactNode;
  onScroll: (event: { nativeEvent: { contentOffset: { y: number } } }) => void; onContentSizeChange: () => void };
let hardwareBack: () => void, listProps: ListProps | undefined, sourceChange: ((value: string) => void) | undefined;
let imageMounts = 0, imageScope: object | undefined, modalOrientations: string[] | undefined;
vi.mock('react-native', () => ({
  Alert: { alert: vi.fn() }, Keyboard: { dismiss: vi.fn() }, Platform: { OS: 'android' }, ActivityIndicator: () => <span role="progressbar" />,
  Text: ({ children, testID }: NativeProps) => <span data-testid={testID}>{children}</span>,
  View: ({ children, testID }: NativeProps) => <div data-testid={testID}>{children}</div>,
  ScrollView: ({ children }: NativeProps) => <div>{children}</div>,
  KeyboardAvoidingView: ({ children, testID }: NativeProps) => <div data-testid={testID}>{children}</div>,
  Modal: ({ children, onRequestClose, supportedOrientations }: NativeProps & { onRequestClose: () => void; supportedOrientations?: string[] }) => { hardwareBack = onRequestClose; modalOrientations = supportedOrientations; return <div role="dialog">{children}</div>; },
  Pressable: ({ children, onPress, accessibilityLabel, disabled, testID }: NativeProps) => <button data-testid={testID} aria-label={accessibilityLabel} onClick={onPress} disabled={disabled}>{children}</button>,
  TextInput: ({ value, onChangeText, accessibilityLabel, editable }: NativeProps & { value: string; onChangeText: (value: string) => void; editable?: boolean }) => {
    if (accessibilityLabel === 'File source') sourceChange = onChangeText;
    return <textarea aria-label={accessibilityLabel} value={value} disabled={editable === false} onChange={event => onChangeText(event.target.value)} />;
  },
  Image: ({ accessibilityLabel }: NativeProps) => <div role="img" aria-label={accessibilityLabel} />,
  FlatList: React.forwardRef((props: ListProps, ref: React.ForwardedRef<{ scrollToOffset: typeof scroll }>) => {
    listProps = props; React.useImperativeHandle(ref, () => ({ scrollToOffset: scroll }));
    useEffect(() => { props.onContentSizeChange(); }, [props.data]);
    return <div>{props.data.length ? props.data.map(item => <div key={item.path}>{props.renderItem({ item })}</div>) : props.ListEmptyComponent}</div>;
  }),
}));
vi.mock('react-native-safe-area-context', () => ({ SafeAreaProvider: ({ children }: NativeProps) => <div>{children}</div>,
  SafeAreaView: ({ children, testID }: NativeProps) => <div data-testid={testID}>{children}</div>, useSafeAreaInsets: () => ({ top: 0, bottom: 0, left: 0, right: 0 }) }));
vi.mock('./ui', () => ({ useUi: () => ({ colors: {}, styles: {} }), Note: ({ children }: NativeProps) => <span>{children}</span> }));
vi.mock('./FilePreview', () => ({ FilePreview: ({ text, kind, preparedMarkdown, wrapLines }: { text: string; kind: string; preparedMarkdown?: string; wrapLines?: boolean }) =>
  <div data-testid="preview" data-kind={kind} data-prepared={preparedMarkdown} data-wrap={wrapLines}>{text}</div> }));
vi.mock('./FilePathSheet', () => ({ FilePathSheet: ({ value, change, submit, close }: { value: string; change: (value: string) => void; submit: (value: string) => void; close: () => void }) =>
  <div data-testid="files-path-sheet"><textarea aria-label="File path" value={value} onChange={event => change(event.target.value)} /><button onClick={() => submit(value)}>Go</button><button onClick={close}>Close path</button></div> }));
vi.mock('./FileMoreMenu', () => ({ FileMoreMenu: ({ items, path, close }: { items: FileMenuItem[]; path: string; close: () => void }) =>
  <div data-testid="files-menu">{path}{items.map(item => <button key={item.label} disabled={item.disabled} onClick={() => { close(); item.action(); }}>{item.label}</button>)}<button onClick={close}>Close menu</button></div> }));
vi.mock('./FileImageEditor', () => ({ FileImageEditor: (props: FileImageEditorProps) => {
  useEffect(() => { imageMounts++; }, []); imageScope = props.draftScope;
  return <div data-testid="image-editor"><button onClick={() => props.onFileAction?.('close')}>Close file</button><button onClick={() => props.onFileAction?.('more')}>More file actions</button>
    <button onClick={() => props.onDirtyChange?.(true)}>Rotate image</button><button onClick={() => { void props.onSave('data:image/png;base64,Yg==').then(result => {
      if (result.ok) { props.onDirtyChange?.(false); props.onSaved?.(result.value, 'data:image/png;base64,Yg=='); }
    }); }}>Save image</button></div>;
} }));
const revision = 'a'.repeat(64);
const textFile: BrowserFile = { kind: 'text', path: '/work/code.ts', mimeType: 'text/plain', size: 4, revision, text: 'code' };
const directory: BrowserDirectory = { path: '/work', parentPath: '/', truncated: false, entries: [
  { name: 'src', path: '/work/src', kind: 'directory' }, { name: 'code.ts', path: '/work/code.ts', kind: 'file' },
] };
function setup(openRequest?: { path: string; line?: number; column?: number }, host = false) {
  const api = { list: vi.fn<FileApi['list']>(async () => success(directory)), read: vi.fn<FileApi['read']>(async () => success(textFile)),
    write: vi.fn<FileApi['write']>(async () => success({ path: textFile.path, revision: 'b'.repeat(64), size: 5 })),
    readAsset: vi.fn<FileApi['readAsset']>(async () => success({ size: 1, mimeType: 'text/javascript', dataUrl: 'data:text/javascript;base64,YQ==' })), writeImage: vi.fn<FileApi['writeImage']>() };
  const onClose = vi.fn();
  const props = { api, ...(host ? {} : { sessionId: 'session', tabId: 'tab' }), available: true, onClose, openRequest };
  const view = render(<FilesScreen {...props} />);
  return { api, onClose, view, props };
}
async function press(name: string) { await act(async () => { fireEvent.click(screen.getByRole('button', { name, exact: true })); }); }
async function menu(name: string) { await press('More file actions'); await press(name); }
async function openFile() { await screen.findByRole('button', { name: 'Open file code.ts' }); await press('Open file code.ts'); }
async function discard() { const buttons = vi.mocked(Alert.alert).mock.calls.at(-1)?.[2]; await act(async () => buttons?.find(button => button.text === 'Discard')?.onPress?.()); }
beforeEach(() => { vi.clearAllMocks(); transfer.supported = false; imageMounts = 0; sourceChange = undefined; imageScope = undefined; });
afterEach(cleanup);

it('shows one compact icon row, no path field or transfer row until requested', async () => {
  setup(); await screen.findByRole('button', { name: 'Open file code.ts' });
  expect(modalOrientations).toEqual(['portrait', 'portrait-upside-down', 'landscape-left', 'landscape-right']);
  expect(within(screen.getByTestId('files-toolbar')).getAllByRole('button')).toHaveLength(5);
  expect(screen.queryByRole('textbox', { name: 'File path' })).toBeNull(); expect(screen.queryByTestId('files-notice')).toBeNull();
  await openFile(); expect(screen.getAllByTestId('files-toolbar')).toHaveLength(1);
  expect(within(screen.getByTestId('files-toolbar')).getAllByRole('button')).toHaveLength(6);
  expect(screen.getByRole('button', { name: 'Close file' })).toHaveTextContent('');
  expect(screen.queryByRole('button', { name: 'Back' })).toBeNull();
});
it('restores the exact origin directory and scroll without RPC, including offline', async () => {
  const f = setup(); await screen.findByRole('button', { name: 'Open file code.ts' });
  act(() => listProps?.onScroll({ nativeEvent: { contentOffset: { y: 120 } } }));
  await openFile(); f.view.rerender(<FilesScreen {...f.props} available={false} />); await press('Close file');
  expect(screen.getByRole('button', { name: 'Open file code.ts' })).toBeInTheDocument();
  expect(scroll).toHaveBeenLastCalledWith({ offset: 120, animated: false }); expect(f.api.list).toHaveBeenCalledTimes(1); expect(f.onClose).not.toHaveBeenCalled();
});
it('closes a direct terminal file link to its caller instead of inventing a parent page', async () => {
  const f = setup({ path: './code.ts' }); await screen.findByTestId('preview'); await press('Close file');
  expect(f.onClose).toHaveBeenCalledOnce(); expect(f.api.list).not.toHaveBeenCalled();
});
it('uses cached history for folder Back and keeps the original session/tab scope', async () => {
  const f = setup(); await screen.findByRole('button', { name: 'Open folder src' });
  f.api.list.mockResolvedValueOnce(success({ ...directory, path: '/work/src', entries: [] })); await press('Open folder src');
  expect(f.api.list).toHaveBeenLastCalledWith({ sessionId: 'session', tabId: 'tab', path: '/work/src' });
  await act(async () => hardwareBack()); expect(screen.getByRole('button', { name: 'Open file code.ts' })).toBeInTheDocument();
  expect(f.api.list).toHaveBeenCalledTimes(2); await press('Back'); expect(f.onClose).toHaveBeenCalledOnce();
});
it('saves revision-checked native source and retains conflicts until explicitly discarded', async () => {
  const f = setup(); await openFile(); await press('Edit source');
  fireEvent.change(screen.getByLabelText('File source'), { target: { value: 'saved' } }); await press('Save');
  expect(f.api.write).toHaveBeenCalledWith({ sessionId: 'session', tabId: 'tab', path: textFile.path, text: 'saved', revision });
  fireEvent.change(screen.getByLabelText('File source'), { target: { value: 'unsaved' } });
  f.api.write.mockResolvedValueOnce(failure('FILE_CONFLICT', 'Changed on host.')); await press('Save');
  expect(f.api.write).toHaveBeenLastCalledWith(expect.objectContaining({ revision: 'b'.repeat(64) }));
  expect(screen.getByLabelText('File source')).toHaveValue('unsaved'); await press('Close file'); expect(Alert.alert).toHaveBeenCalled();
  await discard(); expect(screen.getByRole('button', { name: 'Open file code.ts' })).toBeInTheDocument();
});
it('keeps dirty text when switching to read/Markdown preview and leaves Save last', async () => {
  const f = setup(); f.api.read.mockResolvedValueOnce(success({ ...textFile, path: '/work/readme.md', mimeType: 'text/markdown', text: '# Old' }));
  await openFile(); await press('Edit source'); fireEvent.change(screen.getByLabelText('File source'), { target: { value: '# New' } }); await press('Read file');
  expect(Alert.alert).not.toHaveBeenCalled(); expect(screen.getByTestId('preview')).toHaveTextContent('# New');
  const controls = within(screen.getByTestId('files-toolbar')).getAllByRole('button'); expect(controls).toHaveLength(6); expect(controls.at(-1)).toHaveAccessibleName('Save');
  expect(within(screen.getByTestId('files-toolbar')).queryByRole('button', { name: 'Download selected file' })).toBeNull();
  await menu('Unwrap lines'); expect(screen.getByTestId('preview')).toHaveAttribute('data-wrap', 'false');
});
it('keeps X and hardware Back guarded while a save is in flight', async () => {
  const f = setup(); await openFile(); await press('Edit source');
  let finish!: (value: Awaited<ReturnType<FileApi['write']>>) => void;
  f.api.write.mockReturnValueOnce(new Promise(resolve => { finish = resolve; }));
  fireEvent.change(screen.getByLabelText('File source'), { target: { value: 'saving' } }); await press('Save');
  expect(screen.getByRole('button', { name: 'Close file' })).toBeDisabled(); expect(screen.getByRole('progressbar')).toBeInTheDocument();
  await act(async () => hardwareBack()); expect(f.onClose).not.toHaveBeenCalled(); expect(screen.getByLabelText('File source')).toBeInTheDocument();
  await act(async () => finish(success({ path: textFile.path, revision: 'b'.repeat(64), size: 6 })));
});
it('opens host home without sessions and resets a guest root only after explicit path submission', async () => {
  const f = setup(undefined, true); await screen.findByRole('button', { name: 'Open file code.ts' }); expect(f.api.list).toHaveBeenCalledWith({ path: '~' });
  f.view.unmount(); const guest = setup(); await openFile(); await press('Show file path');
  fireEvent.change(screen.getByLabelText('File path'), { target: { value: '~' } }); await press('Go');
  expect(guest.api.list).toHaveBeenLastCalledWith({ path: '~' }); await openFile(); expect(guest.api.read).toHaveBeenLastCalledWith({ path: '/work/code.ts' });
});
it('path navigation guards dirty source while merely opening Path/More keeps it intact', async () => {
  setup(); await openFile(); await press('Edit source'); const input = screen.getByLabelText('File source');
  fireEvent.change(input, { target: { value: 'unsaved' } }); await press('Show file path'); await press('Close path');
  expect(screen.getByLabelText('File source')).toBe(input); await press('More file actions'); await press('Close menu'); expect(screen.getByLabelText('File source')).toBe(input);
  await press('Show file path'); fireEvent.change(screen.getByLabelText('File path'), { target: { value: '~' } }); await press('Go');
  expect(Alert.alert).toHaveBeenCalled(); expect(screen.getByLabelText('File source')).toHaveValue('unsaved'); expect(Keyboard.dismiss).toHaveBeenCalled();
});
it('copies canonical paths locally even offline and reports clipboard errors without changing the draft', async () => {
  const f = setup({ path: './code.ts', line: 12, column: 3 }); await screen.findByLabelText('File source');
  f.view.rerender(<FilesScreen {...f.props} available={false} />); await press('Copy file path');
  expect(Clipboard.setString).toHaveBeenCalledExactlyOnceWith('/work/code.ts'); expect(screen.getByText('Path copied.')).toBeInTheDocument();
  vi.mocked(Clipboard.setString).mockImplementationOnce(() => { throw new Error('Clipboard unavailable.'); }); await press('Copy file path');
  expect(screen.getByText('Clipboard unavailable.')).toBeInTheDocument(); expect(screen.getByLabelText('File source')).toHaveValue('code');
});
it('uses fullscreen file chrome for preview errors and still permits Download', async () => {
  transfer.supported = true; const f = setup(); await screen.findByRole('button', { name: 'Open file code.ts' });
  f.api.read.mockResolvedValueOnce(failure('UNSUPPORTED', 'Preview too large.')); f.api.list.mockResolvedValueOnce(failure('NOT_FOUND', 'Not a folder.'));
  await press('Open file code.ts'); expect(screen.getByRole('button', { name: 'Close file' })).toBeInTheDocument();
  expect(screen.queryByRole('button', { name: 'Open file code.ts' })).toBeNull(); await press('Download selected file');
  expect(transfer.download).toHaveBeenCalledExactlyOnceWith('/work/code.ts'); await press('Close file'); expect(screen.getByRole('button', { name: 'Open file code.ts' })).toBeInTheDocument();
});
it('resolves Markdown sibling images and HTML scripts through the scoped host API', async () => {
  const f = setup(); f.api.read.mockResolvedValueOnce(success({ ...textFile, path: '/work/README.md', mimeType: 'text/markdown', text: '# Diagram\n\n![image](image.png)' }));
  f.api.readAsset.mockResolvedValueOnce(success({ size: 1, mimeType: 'image/png', dataUrl: 'data:image/png;base64,YQ==' })); await openFile();
  await waitFor(() => expect(screen.getByTestId('preview').getAttribute('data-prepared')).toContain('data:image/png;base64,YQ=='));
  expect(f.api.readAsset).toHaveBeenCalledWith({ sessionId: 'session', tabId: 'tab', path: '/work/README.md', assetPath: 'image.png' });
  await press('Close file'); f.api.read.mockResolvedValueOnce(success({ ...textFile, path: '/work/index.html', mimeType: 'text/html', text: '<script src="local.js"></script>' }));
  await openFile(); await waitFor(() => expect(f.api.readAsset).toHaveBeenCalledWith({ sessionId: 'session', tabId: 'tab', path: '/work/index.html', assetPath: 'local.js' }));
});
it('image edit owns the only toolbar; Path/More preserve canvas and Preview guards unsaved work', async () => {
  const f = setup(); const image: BrowserFile = { kind: 'image', path: '/work/image.png', revision, size: 1, mimeType: 'image/png', dataUrl: 'data:image/png;base64,YQ==' };
  f.api.read.mockResolvedValueOnce(success(image)); f.api.writeImage.mockResolvedValue(success({ path: image.path, revision: 'b'.repeat(64), size: 1 }));
  await openFile(); await press('Edit image'); expect(screen.queryByTestId('files-toolbar')).toBeNull(); expect(imageMounts).toBe(1);
  expect(imageScope).toBe(getImageDraftScope(f.api, 'session', 'tab')); await press('Rotate image'); await menu('Show file path'); await press('Close path');
  expect(imageMounts).toBe(1); await menu('Preview image'); expect(Alert.alert).toHaveBeenCalled(); expect(imageMounts).toBe(1);
  await discard(); expect(discardImageEditorDraft).toHaveBeenCalledWith(image, getImageDraftScope(f.api, 'session', 'tab'));
  await press('Edit image'); await press('Rotate image'); await press('Save image');
  expect(f.api.writeImage).toHaveBeenCalledWith({ sessionId: 'session', tabId: 'tab', path: image.path, revision, dataUrl: 'data:image/png;base64,Yg==' });
});
it('rejects a late native input callback after another file has opened', async () => {
  const f = setup(); await openFile(); await press('Edit source'); const late = sourceChange;
  await press('Close file'); f.api.read.mockResolvedValueOnce(success({ ...textFile, path: '/work/other.ts', text: 'new source' })); await openFile();
  act(() => late?.('late source')); expect(screen.getByTestId('preview')).toHaveTextContent('new source');
});
