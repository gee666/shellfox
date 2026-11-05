import { useCallback, useEffect, useRef, useState } from 'react';
import { imageEditorDocument } from './image-editor-document';
import { acquireImageEditorDraft, completeImageEditorSave, releaseImageEditorDraft, storeImageEditorDraft } from './image-editor-drafts';
import {
  parseImageEditorMessage, validateEditorImage, type EditableImage, type FileImageEditorProps, type ImageEditorCommand, type ImageEditorOperation,
} from './image-editor-protocol';

interface EditorSession {
  token: string;
  scope?: object;
  image: EditableImage;
  expectedSaved?: EditableImage;
  acquired: boolean;
  loadSent: boolean;
  active: boolean;
  ready: boolean;
  dirty: boolean;
  phase: 'idle' | 'encoding' | 'saving';
  operations: ImageEditorOperation[];
  timer?: ReturnType<typeof setTimeout>;
}
interface EditorView { document: string | null; error: string | null; ready: boolean; dirty: boolean; busy: boolean }
const initialView: EditorView = { document: null, error: null, ready: false, dirty: false, busy: false };
function disposeSession(current: EditorSession | null): void {
  if (!current) return;
  current.active = false;
  clearTimeout(current.timer);
  if (current.acquired) releaseImageEditorDraft(current.image, current.scope);
}

/** Shared save/draft lifecycle. Platform wrappers own only their transport and native controls. */
export function useImageEditor(
  props: FileImageEditorProps,
  platform: 'desktop' | 'mobile',
  send: (command: ImageEditorCommand) => void,
  parentOrigin = 'null',
) {
  const [view, setView] = useState(initialView);
  const session = useRef<EditorSession | null>(null);
  const callbacks = useRef(props);
  callbacks.current = props;
  const transport = useRef(send);
  transport.current = send;
  const { path, revision, dataUrl, mimeType } = props.image;
  const draftScope = props.draftScope;
  useEffect(() => {
    const image = callbacks.current.image;
    const previous = session.current;
    if (previous?.active && previous.scope === draftScope && previous.expectedSaved
      && previous.expectedSaved.path === image.path && previous.expectedSaved.revision === image.revision
      && previous.expectedSaved.dataUrl === image.dataUrl && previous.expectedSaved.mimeType === image.mimeType) {
      previous.expectedSaved = undefined;
      return;
    }
    disposeSession(previous);
    // The token separates document generations. Desktop also authenticates the window and origin.
    const token = Array.from({ length: 4 }, () => Math.random().toString(36).slice(2).padEnd(12, '0')).join('');
    const current: EditorSession = { token, scope: draftScope, image, acquired: false, loadSent: false,
      active: true, ready: false, dirty: false, phase: 'idle', operations: [] };
    session.current = current;
    try {
      validateEditorImage(image);
      const operations = acquireImageEditorDraft(image, current.scope);
      current.acquired = true;
      const document = imageEditorDocument({ image, token, platform, parentOrigin, operations });
      current.operations = operations;
      current.dirty = operations.length > 0;
      setView({ ...initialView, document, dirty: current.dirty });
      callbacks.current.onDirtyChange?.(current.dirty);
    } catch (cause) {
      setView({ ...initialView, error: cause instanceof Error ? cause.message : 'Could not open image editor.' });
    }
  }, [path, revision, dataUrl, mimeType, platform, parentOrigin, draftScope]);
  useEffect(() => () => { disposeSession(session.current); session.current = null; }, []);

  const fail = useCallback((message: string) => {
    const current = session.current;
    if (!current?.active) return;
    clearTimeout(current.timer);
    current.phase = 'idle';
    transport.current({ type: 'unlock', token: current.token });
    setView(previous => ({ ...previous, busy: false, error: message }));
  }, []);
  const save = useCallback(() => {
    const current = session.current;
    if (!current?.active || !current.ready || !current.dirty || current.phase !== 'idle') return;
    current.phase = 'encoding';
    setView(previous => ({ ...previous, busy: true, error: null }));
    current.timer = setTimeout(() => {
      if (current.active && current.phase === 'encoding') fail('Image encoding timed out. Try saving again.');
    }, 20_000);
    transport.current({ type: 'export', token: current.token });
  }, [fail]);
  const receive = useCallback((value: unknown) => {
    const current = session.current;
    if (!current?.active) return;
    const message = parseImageEditorMessage(value, current.token);
    if (!message) return;
    if (message.type === 'error') { fail(message.message); return; }
    if (message.type === 'request-image') {
      if (current.loadSent || current.ready) return;
      current.loadSent = true;
      try { transport.current({ type: 'load', token: current.token, dataUrl: current.image.dataUrl }); }
      catch (cause) { fail(cause instanceof Error ? cause.message : 'Could not load image.'); }
      return;
    }
    if (message.type === 'save-request') {
      if (!current.ready || !current.dirty) {
        if (current.phase === 'idle') transport.current({ type: 'unlock', token: current.token });
        return;
      }
      save(); return;
    }
    if (message.type === 'state') {
      if (!current.loadSent || current.phase === 'saving' || message.dirty !== (message.operations.length > 0)) return;
      try { storeImageEditorDraft(current.image, message.operations, current.scope); }
      catch (cause) { fail(cause instanceof Error ? cause.message : 'Could not retain image edits.'); return; }
      current.ready = true;
      current.operations = message.operations;
      current.dirty = message.dirty;
      setView(previous => ({ ...previous, ready: true, dirty: message.dirty }));
      callbacks.current.onDirtyChange?.(message.dirty);
      return;
    }
    if (current.phase !== 'encoding' || !message.dataUrl.startsWith(`data:${current.image.mimeType};base64,`)) return;
    clearTimeout(current.timer);
    current.phase = 'saving';
    const image = current.image;
    const onSave = callbacks.current.onSave;
    void (async () => {
      try {
        const result = await onSave(message.dataUrl);
        if (!result.ok) { if (current.active) fail(result.error.message); return; }
        completeImageEditorSave(image, current.operations, current.scope);
        if (!current.active) return;
        // Move the clean baseline to its saved identity without reloading the canvas.
        releaseImageEditorDraft(image, current.scope); current.acquired = false;
        const nextImage: EditableImage = { ...image, ...result.value, dataUrl: message.dataUrl };
        acquireImageEditorDraft(nextImage, current.scope); current.acquired = true;
        current.image = nextImage; current.expectedSaved = nextImage;
        current.operations = [];
        current.phase = 'idle';
        current.dirty = false;
        setView(previous => ({ ...previous, busy: false, dirty: false, error: null }));
        transport.current({ type: 'saved', token: current.token });
        callbacks.current.onDirtyChange?.(false);
        callbacks.current.onSaved?.(result.value, message.dataUrl);
      } catch (cause) {
        if (current.active) fail(cause instanceof Error ? cause.message : 'Could not save image.');
      }
    })();
  }, [fail, save]);
  return { ...view, receive, fail };
}
