// @vitest-environment jsdom
import React, { type ReactNode, useImperativeHandle } from 'react';
import '@testing-library/jest-dom/vitest';
import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { success } from '../../src/shared/contracts';
import type { EditableImage } from '../../src/shared/image-editor-protocol';
import { FileImageEditor, discardImageEditorDraft } from './FileImageEditor';

interface NativeMessage { nativeEvent: { data: string; url: string } }
interface NativeWebProps {
  source: { html: string; baseUrl: string };
  onMessage: (event: NativeMessage) => void;
  onShouldStartLoadWithRequest: (request: { url: string }) => boolean;
  onNavigationStateChange: (state: { url: string }) => void;
  javaScriptCanOpenWindowsAutomatically: boolean; allowFileAccess: boolean; domStorageEnabled: boolean;
  allowUniversalAccessFromFileURLs: boolean; sharedCookiesEnabled: boolean; mixedContentMode: string;
}
interface WebHandle { postMessage: (message: string) => void; stopLoading: () => void }
let webProps: NativeWebProps | null = null;
const postMessage = vi.fn(), stopLoading = vi.fn();
vi.mock('react-native', () => ({ View: ({ children }: { children?: ReactNode }) => <div>{children}</div> }));
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
function setup() {
  const onSave = vi.fn(async (_dataUrl: string) => success(saved)), onSaved = vi.fn(), onDirtyChange = vi.fn();
  const view = render(<FileImageEditor image={image} onSave={onSave} onSaved={onSaved} onDirtyChange={onDirtyChange} />);
  const web = webProps;
  if (!web) throw new Error('Missing WebView');
  const token = /"token":"([a-zA-Z0-9_-]+)"/.exec(web.source.html)?.[1];
  if (!token) throw new Error('Missing token');
  const send = (data: unknown, url = 'about:blank') => act(() => web.onMessage({ nativeEvent: { data: JSON.stringify(data), url } }));
  send({ type: 'request-image', token });
  const state = { type: 'state', token, width: 3, height: 2, dirty: true, operations: [{ type: 'rotate', direction: 'left' }] };
  const saveRequest = () => send({ type: 'save-request', token });
  return { ...view, web, token, send, state, saveRequest, onSave, onSaved, onDirtyChange };
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
it('restores edits after the native view unmounts', () => {
  const f = setup(); f.send(f.state); f.unmount();
  const reopened = setup(); expect(reopened.web.source.html).toContain('"operations":[{"type":"rotate","direction":"left"}]');
});
