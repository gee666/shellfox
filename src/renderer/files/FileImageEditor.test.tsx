// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { failure, success } from '../../shared/contracts';
import type { EditableImage, FileImageEditorProps } from '../../shared/image-editor-protocol';
import { FileImageEditor, discardImageEditorDraft } from './FileImageEditor';

const image: EditableImage = { kind: 'image', path: '/photo.png', mimeType: 'image/png', revision: 'a'.repeat(64), size: 3, dataUrl: 'data:image/png;base64,AAAA' };
const saved = { path: image.path, revision: 'b'.repeat(64), size: 3 };
afterEach(() => { cleanup(); discardImageEditorDraft(image); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
function setup(override: Partial<FileImageEditorProps> = {}, initialize = true) {
  const onSave = vi.fn(async (_dataUrl: string) => success(saved));
  const onSaved = vi.fn(), onDirtyChange = vi.fn();
  const props = { image, onSave, onSaved, onDirtyChange, ...override };
  const view = render(<FileImageEditor {...props} />);
  const frame = screen.getByTitle('Image editing canvas');
  if (!(frame instanceof HTMLIFrameElement)) throw new Error('Missing image editor');
  const html = new URL(frame.src).searchParams.get('html') ?? '';
  const token = /"token":"([a-zA-Z0-9_-]+)"/.exec(html)?.[1];
  if (!token) throw new Error('Missing document token');
  const post = (value: unknown, origin = 'null', source: Window | null = frame.contentWindow) => act(() => {
    window.dispatchEvent(new MessageEvent('message', { data: value, origin, source }));
  });
  const child = frame.contentWindow; if (!child) throw new Error('Missing frame window');
  const sent = vi.spyOn(child, 'postMessage');
  if (initialize) post({ type: 'request-image', token });
  const state = (dirty: boolean) => post({ type: 'state', token, dirty, width: 3, height: 2, operations: dirty ? [{ type: 'rotate', direction: 'left' }] : [] });
  const exported = () => post({ type: 'export', token, dataUrl: image.dataUrl });
  const saveRequest = () => post({ type: 'save-request', token });
  return { ...view, frame, html, token, post, state, exported, saveRequest, sent, onSave, onSaved, onDirtyChange };
}
it('uses the isolated protocol, opaque sandbox and exact origin/source/token authentication', () => {
  const f = setup();
  expect(f.frame.src).toMatch(/^shellfox-preview:\/\/document\//);
  expect(f.frame.getAttribute('sandbox')).toBe('allow-scripts');
  expect(f.frame).not.toHaveAttribute('srcdoc');
  const state = { type: 'state', token: f.token, dirty: true, width: 3, height: 2, operations: [{ type: 'rotate', direction: 'left' }] };
  f.post(state, 'https://evil.example'); f.post(state, 'null', window); f.post({ ...state, token: 'wrong_editor_token' });
  f.saveRequest(); expect(f.sent).not.toHaveBeenCalledWith({ type: 'export', token: f.token }, '*');
  expect(screen.queryByRole('button', { name: 'Save image' })).not.toBeInTheDocument();
  f.state(true); f.saveRequest(); expect(f.sent).toHaveBeenCalledWith({ type: 'export', token: f.token }, '*');
  expect(f.onDirtyChange).toHaveBeenLastCalledWith(true);
});
it('keeps a >8 MiB original out of the document URL and loads it only after an authenticated request', () => {
  const large: EditableImage = { ...image, size: 9 * 1024 * 1024, dataUrl: 'data:image/png;base64,' + 'A'.repeat(12 * 1024 * 1024) };
  const f = setup({ image: large }, false);
  expect(f.html.length).toBeLessThan(100_000); expect(f.frame.src.length).toBeLessThan(200_000);
  expect(f.html).not.toContain(large.dataUrl);
  const child = f.frame.contentWindow; if (!child) throw new Error('Missing frame');
  const send = vi.spyOn(child, 'postMessage');
  f.post({ type: 'request-image', token: 'foreign_document_token' });
  f.post({ type: 'request-image', token: f.token }, 'https://foreign.example');
  expect(send).not.toHaveBeenCalled();
  f.post({ type: 'request-image', token: f.token }); f.post({ type: 'request-image', token: f.token });
  expect(send).toHaveBeenCalledExactlyOnceWith({ type: 'load', token: f.token, dataUrl: large.dataUrl }, '*');
});
it('does not reload the trusted document when the parent applies a successful save revision', async () => {
  const f = setup(); const originalUrl = f.frame.src;
  f.state(true); f.saveRequest(); f.exported(); await act(async () => {});
  const updated: EditableImage = { ...image, ...saved };
  f.rerender(<FileImageEditor image={updated} onSave={f.onSave} />);
  expect(f.frame.src).toBe(originalUrl);
  f.state(true); expect(f.onDirtyChange).toHaveBeenLastCalledWith(false); f.unmount();
  try { const reopened = setup({ image: updated }); expect(reopened.html).toContain('"operations":[{"type":"rotate","direction":"left"}]'); }
  finally { discardImageEditorDraft(updated); }
});
it('configures the serialized null parent origin for a packaged file location', () => {
  const browserWindow = window;
  const fileLocation = new URL('file:///P:/shellfox/index.html');
  // Electron reports file:// here even though MessageEvent.origin is null.
  Object.defineProperty(fileLocation, 'origin', { value: 'file://' });
  vi.stubGlobal('window', new Proxy(browserWindow, {
    get: (target, key) => key === 'location' ? fileLocation : Reflect.get(target, key, target),
  }));
  const f = setup();
  expect(window.location.protocol).toBe('file:');
  expect(window.location.origin).toBe('file://');
  expect(f.html).toContain('"parentOrigin":"null"');
  expect(f.html).toContain("event.source !== parent || event.origin !== config.parentOrigin");
  expect(f.html).toContain('message.token !== config.token');
});
it('saves only an explicit export, locks duplicate requests and reports the revision plus saved bytes', async () => {
  const f = setup(); f.state(true); f.exported(); expect(f.onSave).not.toHaveBeenCalled();
  f.sent.mockClear(); f.saveRequest(); f.saveRequest();
  expect(f.sent).toHaveBeenCalledExactlyOnceWith({ type: 'export', token: f.token }, '*');
  f.exported(); f.exported();
  await act(async () => {});
  expect(f.onSave).toHaveBeenCalledExactlyOnceWith(image.dataUrl);
  expect(f.onSaved).toHaveBeenCalledExactlyOnceWith(saved, image.dataUrl);
  expect(f.onDirtyChange).toHaveBeenLastCalledWith(false);
  expect(f.sent).toHaveBeenLastCalledWith({ type: 'saved', token: f.token }, '*');
});
it('keeps the dirty draft and reports revision conflicts without retrying or overwriting', async () => {
  const onSave = vi.fn(async () => failure('FILE_CONFLICT', 'Changed on disk. Reopen the image.'));
  const f = setup({ onSave }); f.state(true);
  f.saveRequest(); f.exported();
  await act(async () => {});
  expect(screen.getByRole('alert')).toHaveTextContent('Changed on disk');
  expect(f.sent).toHaveBeenLastCalledWith({ type: 'unlock', token: f.token }, '*');
  expect(f.onSaved).not.toHaveBeenCalled(); expect(onSave).toHaveBeenCalledOnce();
  f.unmount();
  const reopened = setup();
  expect(reopened.html).toContain('"operations":[{"type":"rotate","direction":"left"}]');
  expect(reopened.onDirtyChange).toHaveBeenLastCalledWith(true);
});
it('restores dirty operations after unmount and removes them only on explicit discard', () => {
  const f = setup(); f.state(true); f.unmount();
  const restored = setup(); expect(restored.html).toContain('"operations":[{"type":"rotate","direction":"left"}]'); restored.unmount();
  discardImageEditorDraft(image);
  const cleared = setup(); expect(cleared.html).toContain('"operations":[]');
});
it('keeps the global unload guard after a dirty editor unmounts until its scoped draft is discarded', () => {
  const hostA = {}, hostB = {};
  const unloadPrevented = () => {
    const event = new Event('beforeunload', { cancelable: true });
    window.dispatchEvent(event);
    return event.defaultPrevented;
  };
  try {
    expect(unloadPrevented()).toBe(false);
    const first = setup({ draftScope: hostA }); first.state(true); first.unmount();
    expect(unloadPrevented()).toBe(true);
    const second = setup({ draftScope: hostB });
    expect(second.html).toContain('"operations":[]');
    second.state(true); second.unmount();
    discardImageEditorDraft(image, hostA);
    expect(unloadPrevented()).toBe(true);
    const restored = setup({ draftScope: hostB });
    expect(restored.html).toContain('"operations":[{"type":"rotate","direction":"left"}]'); restored.unmount();
    discardImageEditorDraft(image, hostB);
    expect(unloadPrevented()).toBe(false);
  } finally { discardImageEditorDraft(image, hostA); discardImageEditorDraft(image, hostB); }
});
it('ignores messages from a previous document when the original image changes', () => {
  const f = setup();
  f.rerender(<FileImageEditor image={{ ...image, revision: 'c'.repeat(64) }} onSave={f.onSave} />);
  f.state(true); f.saveRequest(); expect(f.onSave).not.toHaveBeenCalled();
});
