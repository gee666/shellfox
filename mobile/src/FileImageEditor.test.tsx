// @vitest-environment jsdom
import React, { type ReactNode, useImperativeHandle, useState } from 'react';
import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { success } from '../../src/shared/contracts';
import type { EditableImage, FileImageEditorProps } from '../../src/shared/image-editor-protocol';
import { FILE_IMAGE_BYTES } from '../../src/shared/file-schemas';
import { FileImageEditor, discardImageEditorDraft, getImageDraftScope } from './FileImageEditor';

interface NativeMessage { nativeEvent: { data: string; url: string } }
interface NativeWebProps {
  source: { html: string; baseUrl: string };
  onMessage: (event: NativeMessage) => void;
  onShouldStartLoadWithRequest: (request: { url: string }) => boolean;
  onNavigationStateChange: (state: { url: string }) => void;
  onError: (event: { nativeEvent: { description: string } }) => void;
  onContentProcessDidTerminate: () => void;
  onRenderProcessGone: () => void;
  javaScriptCanOpenWindowsAutomatically: boolean; allowFileAccess: boolean; domStorageEnabled: boolean;
  allowUniversalAccessFromFileURLs: boolean; sharedCookiesEnabled: boolean; mixedContentMode: string;
}
interface WebHandle { postMessage: (message: string) => void; stopLoading: () => void }
let webProps: NativeWebProps | null = null;
const postMessage = vi.fn(), stopLoading = vi.fn();
vi.mock('react-native', () => ({
  View: ({ children, testID, style, pointerEvents }: { children?: ReactNode; testID?: string; style?: unknown; pointerEvents?: string }) =>
    <div data-testid={testID} data-native-style={JSON.stringify(style)} data-pointer-events={pointerEvents}>{children}</div>,
  Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
}));
vi.mock('./FileIconButton', () => ({ FileIconButton: ({ label, onPress, disabled }: { label: string; onPress: () => void; disabled?: boolean }) =>
  <button aria-label={label} disabled={disabled} onClick={onPress} /> }));
vi.mock('./ui', () => ({
  useUi: () => ({ colors: {}, styles: {} }),
  Note: ({ children, error }: { children: ReactNode; error?: boolean }) => <span role={error ? 'alert' : 'status'}>{children}</span>,
}));
vi.mock('react-native-webview', () => ({ WebView: React.forwardRef<WebHandle, NativeWebProps>((props, ref) => {
  webProps = props; useImperativeHandle(ref, () => ({ postMessage, stopLoading }));
  return <div data-testid="native-image-editor" />;
}) }));
const image: EditableImage = { kind: 'image', path: '/photo.png', mimeType: 'image/png', revision: 'a'.repeat(64), size: 3, dataUrl: 'data:image/png;base64,AAAA' };
const saved = { path: image.path, revision: 'b'.repeat(64), size: 3 };
afterEach(() => { cleanup(); discardImageEditorDraft(image); webProps = null; vi.clearAllMocks(); });
function currentWeb(): NativeWebProps {
  if (!webProps) throw new Error('Missing WebView');
  return webProps;
}
function setup(override: Partial<FileImageEditorProps> = {}) {
  const onSave = vi.fn(async (_dataUrl: string) => success(saved)), onSaved = vi.fn(), onDirtyChange = vi.fn();
  const props: FileImageEditorProps = { image, onSave, onSaved, onDirtyChange, ...override };
  const view = render(<FileImageEditor {...props} />);
  const web = webProps;
  if (!web) throw new Error('Missing WebView');
  const token = /"token":"([a-zA-Z0-9_-]+)"/.exec(web.source.html)?.[1];
  if (!token) throw new Error('Missing token');
  const send = (data: unknown, url = 'about:blank') => act(() => web.onMessage({ nativeEvent: { data: JSON.stringify(data), url } }));
  send({ type: 'request-image', token });
  const state = { type: 'state', token, width: 3, height: 2, dirty: true, operations: [{ type: 'rotate', direction: 'left' }] };
  const saveRequest = () => send({ type: 'save-request', token });
  return { ...view, props, web, token, send, state, saveRequest, onSave, onSaved, onDirtyChange };
}
it('blocks native navigation/file access/storage and accepts only its document bridge', () => {
  const f = setup();
  expect(f.web.source.baseUrl).toBe('about:blank');
  expect(f.web).toMatchObject({ javaScriptCanOpenWindowsAutomatically: false, allowFileAccess: false, domStorageEnabled: false,
    allowUniversalAccessFromFileURLs: false, sharedCookiesEnabled: false, mixedContentMode: 'never' });
  for (const url of ['https://evil.example', 'file:///etc/passwd', 'data:text/html,attack', 'javascript:attack()']) {
    expect(f.web.onShouldStartLoadWithRequest({ url })).toBe(false);
  }
  f.web.onNavigationStateChange({ url: 'https://evil.example' }); expect(stopLoading).toHaveBeenCalledOnce();
  f.send({ ...f.state, token: 'other_editor_token' }, 'https://evil.example'); f.send({ ...f.state, token: 'other_editor_token' });
  f.saveRequest(); expect(postMessage).not.toHaveBeenCalledWith(JSON.stringify({ type: 'export', token: f.token }));
  expect(screen.queryByRole('button', { name: 'Save image' })).not.toBeInTheDocument();
  f.send(f.state); f.saveRequest(); expect(postMessage).toHaveBeenLastCalledWith(JSON.stringify({ type: 'export', token: f.token }));
});
it.each(['null', 'data:text/html;base64,PGh0bWw+'])('accepts native inline URL %s only with the current document token and schema', url => {
  const f = setup();
  f.send({ ...f.state, token: 'another_document_token' }, url);
  f.send({ ...f.state, unexpected: 'privileged command' }, url);
  expect(f.onDirtyChange).toHaveBeenLastCalledWith(false);
  f.send(f.state, url);
  expect(f.onDirtyChange).toHaveBeenLastCalledWith(true);
  expect(f.onSave).not.toHaveBeenCalled();
  expect(f.web.onShouldStartLoadWithRequest({ url })).toBe(false);
});
it('uses narrow export commands and reports saved data/revision only after API success', async () => {
  const f = setup(); f.send(f.state);
  f.saveRequest(); f.saveRequest();
  expect(JSON.parse(postMessage.mock.calls.at(-1)?.[0] ?? '{}')).toEqual({ type: 'export', token: f.token });
  expect(f.onSave).not.toHaveBeenCalled();
  f.send({ type: 'export', token: f.token, dataUrl: image.dataUrl });
  await act(async () => {});
  expect(f.onSave).toHaveBeenCalledExactlyOnceWith(image.dataUrl);
  expect(f.onSaved).toHaveBeenCalledExactlyOnceWith(saved, image.dataUrl);
  expect(f.onDirtyChange).toHaveBeenLastCalledWith(false);
  expect(postMessage).toHaveBeenLastCalledWith(JSON.stringify({ type: 'saved', token: f.token }));
});
it('routes mobile file actions to the latest handler without recreating the trusted document', () => {
  const first = vi.fn(), next = vi.fn();
  const f = setup({ onFileAction: first }); const source = f.web.source, canvas = screen.getByTestId('native-image-editor');
  expect(source.html).toContain('id="close"'); expect(source.html).toContain('id="more"');
  expect(screen.queryByTestId('image-editor-fallback-toolbar')).not.toBeInTheDocument();
  f.send({ type: 'file-action', action: 'more', token: 'stale_document_token' });
  f.send({ type: 'file-action', action: 'copy', token: f.token });
  f.send({ type: 'file-action', action: 'more', token: f.token, path: '/other.png' });
  expect(first).not.toHaveBeenCalled();
  f.send({ type: 'file-action', action: 'more', token: f.token }); expect(first).toHaveBeenCalledExactlyOnceWith('more');
  f.rerender(<FileImageEditor {...f.props} onFileAction={next} />);
  expect(currentWeb().source).toBe(source); expect(screen.getByTestId('native-image-editor')).toBe(canvas);
  f.send({ type: 'file-action', action: 'close', token: f.token }); expect(next).toHaveBeenCalledExactlyOnceWith('close');
  expect(first).toHaveBeenCalledOnce();
});
it('rejects Close and More during encoding and API saving, then restores them after success', async () => {
  let finish: () => void = () => { throw new Error('Write not initialized'); };
  const pending = new Promise<void>(resolve => { finish = resolve; });
  const onFileAction = vi.fn();
  const f = setup({ onFileAction, onSave: async () => { await pending; return success(saved); } });
  const action = () => { for (const value of ['close', 'more']) f.send({ type: 'file-action', action: value, token: f.token }); };
  f.send(f.state); f.saveRequest(); action(); expect(onFileAction).not.toHaveBeenCalled();
  f.send({ type: 'export', token: f.token, dataUrl: image.dataUrl }); action(); expect(onFileAction).not.toHaveBeenCalled();
  await act(async () => { finish(); });
  f.send({ type: 'file-action', action: 'more', token: f.token }); expect(onFileAction).toHaveBeenCalledExactlyOnceWith('more');
});
it('preserves the canvas view and document when More opens a clipboard menu and the parent rerenders', () => {
  const copy = vi.fn();
  function Parent() {
    const [menu, setMenu] = useState(false), [copied, setCopied] = useState(false);
    return <><FileImageEditor image={image} onSave={async () => success(saved)} onFileAction={action => setMenu(action === 'more')} />
      {menu && <button onClick={() => { copy(image.path); setCopied(true); setMenu(false); }}>Copy file path</button>}
      {copied && <span>Copied</span>}</>;
  }
  render(<Parent />);
  const source = currentWeb().source, canvas = screen.getByTestId('native-image-editor');
  const token = /"token":"([a-zA-Z0-9_-]+)"/.exec(source.html)?.[1]; if (!token) throw new Error('Missing token');
  act(() => currentWeb().onMessage({ nativeEvent: { url: 'null', data: JSON.stringify({ type: 'file-action', action: 'more', token }) } }));
  fireEvent.click(screen.getByRole('button', { name: 'Copy file path' }));
  expect(copy).toHaveBeenCalledExactlyOnceWith(image.path); expect(screen.getByText('Copied')).toBeVisible();
  expect(currentWeb().source).toBe(source); expect(screen.getByTestId('native-image-editor')).toBe(canvas);
});
const invalidImages: EditableImage[] = [
  { ...image, mimeType: 'image/gif', dataUrl: 'data:image/gif;base64,AAAA' },
  { ...image, size: FILE_IMAGE_BYTES + 1 },
];
it.each(invalidImages)('keeps native fallback Close and More available if validation prevents a document for $mimeType/$size', invalid => {
  const onFileAction = vi.fn();
  render(<FileImageEditor image={invalid} onSave={async () => success(saved)} onFileAction={onFileAction} />);
  expect(screen.getByTestId('image-editor-fallback-toolbar')).toBeVisible(); expect(screen.getByText('photo.png')).toBeVisible();
  expect(screen.getByRole('alert')).toBeVisible(); expect(screen.queryByTestId('native-image-editor')).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'Close file' })); fireEvent.click(screen.getByRole('button', { name: 'More file actions' }));
  expect(onFileAction.mock.calls).toEqual([['close'], ['more']]);
});
it('overlays ordinary renderer/codec errors without replacing the live child toolbar or resizing its view', () => {
  const f = setup({ onFileAction: vi.fn() }), canvas = screen.getByTestId('native-image-editor');
  f.send({ type: 'error', token: f.token, message: 'Could not decode image.' });
  const overlay = screen.getByTestId('image-editor-error-overlay');
  expect(overlay).toHaveAttribute('data-pointer-events', 'none');
  expect(overlay.getAttribute('data-native-style')).toContain('"position":"absolute"'); expect(overlay.getAttribute('data-native-style')).toContain('"maxHeight":"50%"');
  expect(screen.getByTestId('native-image-editor')).toBe(canvas); expect(currentWeb().source).toBe(f.web.source);
  expect(screen.queryByTestId('image-editor-fallback-toolbar')).not.toBeInTheDocument();
});
const fatalCallbacks: Array<'onRenderProcessGone' | 'onContentProcessDidTerminate'> = ['onRenderProcessGone', 'onContentProcessDidTerminate'];
it.each(fatalCallbacks)('provides a guarded native fallback and nondestructive fresh-session retry after %s', event => {
  const scope = getImageDraftScope({}, 'session', 'tab'), onFileAction = vi.fn();
  const f = setup({ draftScope: scope, onFileAction }); f.send(f.state);
  try {
    act(() => f.web[event]());
    expect(screen.queryByTestId('native-image-editor')).not.toBeInTheDocument(); expect(screen.getByTestId('image-editor-fallback-toolbar')).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Save image' })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Close file' })); fireEvent.click(screen.getByRole('button', { name: 'More file actions' }));
    expect(onFileAction.mock.calls).toEqual([['close'], ['more']]);
    f.saveRequest(); f.send({ type: 'export', token: f.token, dataUrl: image.dataUrl }); expect(f.onSave).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Reopen image editor' }));
    const next = currentWeb(), nextToken = /"token":"([a-zA-Z0-9_-]+)"/.exec(next.source.html)?.[1];
    expect(nextToken).toBeTruthy(); expect(nextToken).not.toBe(f.token);
    expect(next.source.html).toContain('"operations":[{"type":"rotate","direction":"left"}]');
    expect(screen.queryByTestId('image-editor-fallback-toolbar')).not.toBeInTheDocument();
    postMessage.mockClear();
    act(() => next.onMessage({ nativeEvent: { url: 'null', data: JSON.stringify({ type: 'request-image', token: nextToken }) } }));
    expect(postMessage).toHaveBeenCalledExactlyOnceWith(JSON.stringify({ type: 'load', token: nextToken, dataUrl: image.dataUrl }));
    expect(f.onDirtyChange).toHaveBeenLastCalledWith(true); expect(f.onSave).not.toHaveBeenCalled();
  } finally { f.unmount(); discardImageEditorDraft(image, scope); }
});
it('aborts encoding after process loss so retry is available, without starting an API write', () => {
  const f = setup({ onFileAction: vi.fn() }); f.send(f.state); f.saveRequest();
  act(() => f.web.onRenderProcessGone());
  expect(screen.getByRole('button', { name: 'Reopen image editor' })).toBeEnabled();
  f.send({ type: 'export', token: f.token, dataUrl: image.dataUrl }); expect(f.onSave).not.toHaveBeenCalled();
});
it('retains an in-flight API save through process death, applies its ACK once, and retries with the saved bytes', async () => {
  let finish: () => void = () => { throw new Error('Write not initialized'); };
  const pending = new Promise<void>(resolve => { finish = resolve; }), edited = 'data:image/png;base64,AQID';
  const write = vi.fn(async (_dataUrl: string) => { await pending; return success(saved); }), ack = vi.fn(), dirty = vi.fn();
  function Parent() {
    const [current, setCurrent] = useState<EditableImage>(image);
    return <><FileImageEditor image={current} onSave={write} onDirtyChange={dirty} onFileAction={() => undefined}
      onSaved={(result, dataUrl) => { ack(result, dataUrl); setCurrent(previous => ({ ...previous, ...result, dataUrl })); }} />
      <span data-testid="revision">{current.revision}</span></>;
  }
  render(<Parent />);
  const web = currentWeb(), token = /"token":"([a-zA-Z0-9_-]+)"/.exec(web.source.html)?.[1]; if (!token) throw new Error('Missing token');
  const send = (value: unknown) => act(() => web.onMessage({ nativeEvent: { url: 'null', data: JSON.stringify(value) } }));
  send({ type: 'request-image', token }); send({ type: 'state', token, dirty: true, width: 3, height: 2, operations: [{ type: 'rotate', direction: 'left' }] });
  send({ type: 'save-request', token }); send({ type: 'export', token, dataUrl: edited });
  expect(write).toHaveBeenCalledExactlyOnceWith(edited);
  act(() => web.onContentProcessDidTerminate());
  for (const label of ['Close file', 'More file actions', 'Reopen image editor']) expect(screen.getByRole('button', { name: label })).toBeDisabled();
  fireEvent.click(screen.getByRole('button', { name: 'Reopen image editor' })); expect(screen.queryByTestId('native-image-editor')).not.toBeInTheDocument();
  expect(ack).not.toHaveBeenCalled(); expect(dirty).toHaveBeenLastCalledWith(true);
  await act(async () => { finish(); });
  expect(ack).toHaveBeenCalledExactlyOnceWith(saved, edited); expect(dirty).toHaveBeenLastCalledWith(false); expect(screen.getByTestId('revision')).toHaveTextContent(saved.revision);
  expect(screen.getByRole('button', { name: 'Reopen image editor' })).toBeEnabled(); expect(screen.queryByTestId('native-image-editor')).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'Reopen image editor' }));
  const recovered = currentWeb(), nextToken = /"token":"([a-zA-Z0-9_-]+)"/.exec(recovered.source.html)?.[1]; expect(nextToken).not.toBe(token);
  expect(recovered.source.html).toContain('"operations":[]');
  act(() => recovered.onMessage({ nativeEvent: { url: 'null', data: JSON.stringify({ type: 'request-image', token: nextToken }) } }));
  expect(postMessage).toHaveBeenLastCalledWith(JSON.stringify({ type: 'load', token: nextToken, dataUrl: edited }));
  expect(write).toHaveBeenCalledOnce(); expect(ack).toHaveBeenCalledOnce();
});
it('recovers a fatal view when a new image generation supplies a fresh document', () => {
  const f = setup({ onFileAction: vi.fn() }); act(() => f.web.onRenderProcessGone());
  const nextImage: EditableImage = { ...image, revision: 'c'.repeat(64), dataUrl: 'data:image/png;base64,AQID' };
  f.rerender(<FileImageEditor {...f.props} image={nextImage} />);
  expect(screen.getByTestId('native-image-editor')).toBeVisible(); expect(screen.queryByTestId('image-editor-fallback-toolbar')).not.toBeInTheDocument();
  const recovered = currentWeb(), token = /"token":"([a-zA-Z0-9_-]+)"/.exec(recovered.source.html)?.[1]; expect(token).not.toBe(f.token);
  act(() => recovered.onMessage({ nativeEvent: { url: 'null', data: JSON.stringify({ type: 'request-image', token }) } }));
  expect(postMessage).toHaveBeenLastCalledWith(JSON.stringify({ type: 'load', token, dataUrl: nextImage.dataUrl }));
});
it('restores edits after the native view unmounts', () => {
  const f = setup(); f.send(f.state); f.unmount();
  const reopened = setup(); expect(reopened.web.source.html).toContain('"operations":[{"type":"rotate","direction":"left"}]');
});
