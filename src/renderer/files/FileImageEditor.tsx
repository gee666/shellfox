import { useCallback, useEffect, useMemo, useRef } from 'react';
import type { FileImageEditorProps, ImageEditorCommand } from '../../shared/image-editor-protocol';
import { useImageEditor } from '../../shared/use-image-editor';
import { desktopPreviewUrl } from '../../shared/file-preview-url';
import './image-editor-unload';

export type { FileImageEditorProps } from '../../shared/image-editor-protocol';
export { discardImageEditorDraft, getImageDraftScope } from '../../shared/image-editor-drafts';

export function FileImageEditor(props: FileImageEditorProps) {
  const frame = useRef<HTMLIFrameElement>(null);
  const send = useCallback((command: ImageEditorCommand) => {
    // The child has an opaque origin. Its receiver validates our origin, source and token.
    frame.current?.contentWindow?.postMessage(command, '*');
  }, []);
  // Chromium serializes file: senders as an opaque "null" message origin.
  const parentOrigin = window.location.protocol === 'file:' ? 'null' : window.location.origin;
  const editor = useImageEditor(props, 'desktop', send, parentOrigin);
  const receive = editor.receive;
  const source = useMemo(() => {
    if (!editor.document) return { url: null, error: null };
    try { return { url: desktopPreviewUrl(editor.document), error: null }; }
    catch (cause) { return { url: null, error: cause instanceof Error ? cause.message : 'Could not open image editor.' }; }
  }, [editor.document]);
  useEffect(() => {
    const onMessage = (event: MessageEvent<unknown>) => {
      if (event.origin !== 'null' || !frame.current?.contentWindow || event.source !== frame.current.contentWindow) return;
      receive(event.data);
    };
    window.addEventListener('message', onMessage);
    return () => window.removeEventListener('message', onMessage);
  }, [receive]);
  return <section aria-label="Image editor" style={{ display: 'flex', flexDirection: 'column', flex: 1, minHeight: 0, height: '100%' }}>
    {(editor.error || source.error) && <p className="file-feedback" role="alert">{editor.error ?? source.error}</p>}
    {source.url && <iframe ref={frame} title="Image editing canvas" sandbox="allow-scripts"
      src={source.url} referrerPolicy="no-referrer"
      style={{ flex: 1, minHeight: 280, width: '100%', border: 0 }} />}
  </section>;
}
