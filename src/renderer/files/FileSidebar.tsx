import { useEffect, useRef, useState } from 'react';
import { failure, type ManagerApi } from '../../shared/contracts';
import { Icon } from '../components';
import { FileTransfers } from './FileTransfers';
import { FileCodeEditor } from './FileCodeEditor';
import { CopyFilePathButton } from './CopyFilePathButton';
import { FileTree } from './FileTree';
import type { FileDirectoryRequest } from './file-directory-request';
import { FileImagePreview, HtmlFilePreview, MarkdownFilePreview } from './FilePreview';
import { fileDirectory, fileName, isHtmlPreviewFile, isMarkdownPreviewFile, isSvgPreviewFile } from './file-preview-utils';
import { useFileDocument, type FileOpenRequest } from './use-file-document';
import { fileApiScopeId, rememberedFile } from './file-drafts';
import { FileImageEditor, discardImageEditorDraft, getImageDraftScope } from './FileImageEditor';
import { LayoutResizer } from './file-layout-resizer';
import { FILE_BROWSER_HEIGHT_DEFAULT, FILE_WIDTH_DEFAULT, useFileLayout } from './use-file-layout';
import './files.css';
import './file-layout.css';

export type { FileOpenRequest } from './use-file-document';
export interface FileSidebarProps {
  api: ManagerApi;
  sessionId?: string;
  initialDirectory?: string;
  allowNativeBrowse?: boolean;
  onHostDirectory?: (path: string) => void;
  tabId?: string;
  openRequest?: FileOpenRequest | null;
  directoryRequest?: FileDirectoryRequest;
  onClose: () => void;
  hidden?: boolean;
  onDirtyChange?: (dirty: boolean) => void;
}

export function FileSidebar(props: FileSidebarProps) {
  return <FileSidebarContent key={JSON.stringify([fileApiScopeId(props.api), props.sessionId, props.tabId ?? null])} {...props} />;
}

function FileSidebarContent({ api, sessionId, tabId, openRequest, onClose, hidden, onDirtyChange, initialDirectory, allowNativeBrowse, onHostDirectory, directoryRequest }: FileSidebarProps) {
  const document = useFileDocument(api, sessionId, tabId);
  const layout = useFileLayout(hidden);
  const imageDraftScope = getImageDraftScope(api.files ?? api, sessionId, tabId);
  const [preview, setPreview] = useState(true);
  const [browser, setBrowser] = useState(true);
  const browserBeforeMaximize = useRef<boolean | null>(null);
  useEffect(() => { if (layout.maximized && browser) browserBeforeMaximize.current = null; }, [layout.maximized, browser]);
  const toggleMaximized = () => {
    if (!layout.maximized) { browserBeforeMaximize.current = browser; setBrowser(false); }
    else { if (browserBeforeMaximize.current !== null) setBrowser(browserBeforeMaximize.current); browserBeforeMaximize.current = null; }
    layout.toggleMaximized();
  };
  useEffect(() => { if (directoryRequest) setBrowser(true); }, [directoryRequest?.id]);
  const [folderPath, setFolderPath] = useState(initialDirectory ?? (sessionId ? '.' : '~'));
  const [folderRefresh, setFolderRefresh] = useState(0);
  const [imageEditing, setImageEditing] = useState(false);
  const [imageDirty, setImageDirty] = useState(false);
  const imageDirtyRef = useRef(false);
  const updateImageDirty = (dirty: boolean) => { imageDirtyRef.current = dirty; setImageDirty(dirty); };
  const [pending, setPending] = useState<(() => void) | null>(null);
  const serial = useRef(0);
  const dialog = useRef<HTMLDivElement>(null);
  const latest = useRef(document);
  latest.current = document;
  const guard = (action: () => void) => {
    if (latest.current.saving) return;
    if (latest.current.dirty || imageDirtyRef.current) setPending(() => action);
    else action();
  };
  const requestOpen = (request: FileOpenRequest) => {
    if (request.path === latest.current.file?.path && (latest.current.dirty || imageDirtyRef.current || latest.current.saving)) {
      setPreview(!request.line);
      latest.current.reveal(request);
      return;
    }
    guard(() => { setPreview(!request.line); setImageEditing(false); updateImageDirty(false); void latest.current.open(request); });
  };
  const requestHandler = useRef(requestOpen);
  requestHandler.current = requestOpen;
  useEffect(() => {
    const previous = rememberedFile(api, sessionId, tabId);
    if (openRequest) requestHandler.current(openRequest);
    else if (previous) requestHandler.current({ path: previous, id: 'restore' });
  }, [openRequest?.id, api, sessionId, tabId]);
  useEffect(() => {
    if (!pending) return;
    const previous = window.document.activeElement;
    const root = dialog.current;
    root?.querySelector('button')?.focus();
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !latest.current.saving) { event.preventDefault(); setPending(null); }
      if (event.key !== 'Tab' || !root) return;
      const buttons = [...root.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')];
      const first = buttons[0], last = buttons.at(-1);
      if (event.shiftKey && window.document.activeElement === first) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && window.document.activeElement === last) { event.preventDefault(); first?.focus(); }
    };
    root?.addEventListener('keydown', handleKey);
    return () => { root?.removeEventListener('keydown', handleKey); if (previous instanceof HTMLElement) previous.focus(); };
  }, [pending]);
  useEffect(() => { onDirtyChange?.(document.dirty || imageDirty); }, [document.dirty, imageDirty, onDirtyChange]);
  const openPath = (path: string) => requestOpen({ path, id: `browser-${++serial.current}` });
  const file = document.file;
  const draft = document.draft;
  const transferScope = !file && document.request?.sessionId
    ? { sessionId: document.request.sessionId, ...(document.request.tabId ? { tabId: document.request.tabId } : {}) }
    : { ...(sessionId ? { sessionId } : {}), ...(tabId ? { tabId } : {}) };
  const renderable = !!draft && (isMarkdownPreviewFile(draft.path) || isHtmlPreviewFile(draft.path) || isSvgPreviewFile(draft.path));
  return <aside ref={layout.root} className="file-sidebar" aria-label="File sidebar" hidden={hidden}
    data-maximized={layout.maximized} data-narrow={layout.narrow} style={layout.style}>
    <LayoutResizer className="file-width-resizer" label="File sidebar width" orientation="vertical" reverse
      value={layout.width} min={layout.widthMin} max={layout.widthMax} defaultValue={FILE_WIDTH_DEFAULT}
      onChange={layout.setWidth} hidden={hidden || layout.maximized || layout.narrow} />
    <header className="file-sidebar-header">
      <button type="button" className="file-browser-toggle" aria-label="Toggle file browser" title={browser ? 'Collapse file browser' : 'Expand file browser'} aria-expanded={browser} aria-pressed={browser} onClick={() => setBrowser(value => !value)}><Icon name="chevron" /></button>
      <span title={file?.path ?? document.request?.path}>{file ? fileName(file.path) : 'Files'}{document.dirty || imageDirty ? ' *' : ''}</span>
      {file && <button type="button" className="file-maximize-toggle icon-button" aria-label={layout.maximized ? 'Restore file' : 'Maximize file'}
        title={layout.maximized ? 'Restore file' : 'Maximize file'} aria-pressed={layout.maximized} onClick={toggleMaximized}>
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3" aria-hidden="true">
          {layout.maximized ? <path d="M5 5V2h9v9h-3M2 5h9v9H2z" /> : <path d="M6 2H2v4m8-4h4v4M2 10v4h4m8-4v4h-4" />}
        </svg>
      </button>}
      <button type="button" aria-label="Close file sidebar" title="Close file sidebar" onClick={() => guard(onClose)}>×</button>
    </header>
    {!api.files ? <p role="status" className="file-feedback">File browsing is not supported by this host.</p> : <>
      {browser && <FileTree api={api} sessionId={sessionId} tabId={tabId} selectedPath={file?.path} onOpen={openPath}
        initialDirectory={folderPath} onFolderChange={setFolderPath} allowNativeBrowse={allowNativeBrowse} hidden={hidden} refreshId={folderRefresh} onHostDirectory={onHostDirectory}
        directoryRequest={directoryRequest ?? (file && document.request?.sessionId ? { path: fileDirectory(file.path), id: document.request.id } : undefined)} />}
      <LayoutResizer className="file-browser-resizer" label="File browser height" orientation="horizontal"
        value={layout.browserHeight} min={layout.heightMin} max={layout.heightMax} defaultValue={FILE_BROWSER_HEIGHT_DEFAULT}
        onChange={layout.setBrowserHeight} hidden={hidden || !browser} />
      <FileTransfers api={api} location={{ ...transferScope, path: folderPath }}
        selectedPath={file?.path ?? document.request?.path} onUploaded={() => setFolderRefresh(value => value + 1)} />
      <section className="file-document" aria-label="File preview and editor">
        {document.loading && <p className="file-feedback" role="status">Loading file…</p>}
        {document.error && <div className="file-feedback" role="alert">{document.error}
          {!file && document.request && <button onClick={() => { if (document.request) void document.open(document.request); }}>Retry file</button>}
        </div>}
        {draft?.conflict && <p className="file-feedback" role="alert">This file changed on disk. Your draft is retained. Copy it before reloading; saving will check for conflicts.</p>}
        {file && <div className="file-document-toolbar">
          <span className="file-document-path" title={file.path}>{file.path}</span>
          <CopyFilePathButton api={api} path={file.path} />
          {file.kind === 'image' && /^(image\/(?:png|jpeg|webp))$/.test(file.mimeType) && <button type="button" className="icon-button" aria-pressed={imageEditing}
            aria-label={imageEditing ? 'Preview image' : 'Edit image'} title={imageEditing ? 'Preview image' : 'Edit image'}
            onClick={() => guard(() => setImageEditing(value => !value))}><Icon name={imageEditing ? 'eye' : 'edit'} /></button>}
          {renderable && <>
            <button type="button" aria-pressed={!preview} onClick={() => setPreview(false)}>Source</button>
            <button type="button" aria-pressed={preview} onClick={() => setPreview(true)}>Preview</button>
          </>}
          {draft && <button type="button" aria-label="Save file" disabled={!document.dirty || document.saving}
            onClick={() => void document.save()}>{document.saving ? 'Saving…' : 'Save'}</button>}
          <button type="button" aria-label="Reload file" disabled={document.saving}
            onClick={() => guard(() => void document.open({ path: file.path, id: `reload-${++serial.current}` }))}>↻</button>
        </div>}
        {!document.loading && !file && !document.error && <p className="file-feedback">Select a file.</p>}
        {file?.kind === 'image' && (imageEditing
          ? <><FileImageEditor image={file} draftScope={imageDraftScope} onDirtyChange={updateImageDirty} onSaved={document.updateImage}
            onSave={async dataUrl => {
              if (!api.files) return failure('UNSUPPORTED', 'Image editing is not supported by this host.');
              return api.files.writeImage({ ...(sessionId ? { sessionId } : {}), ...(tabId ? { tabId } : {}), path: file.path, revision: file.revision, dataUrl });
            }} /></>
          : <FileImagePreview key={file.path + file.revision} dataUrl={file.dataUrl} path={file.path} />)}
        {file?.kind === 'binary' && <p className="file-feedback" role="status">Binary file. No preview available. {file.size.toLocaleString()} bytes.</p>}
        {draft && (renderable && preview
          ? isSvgPreviewFile(draft.path)
            ? <FileImagePreview key={draft.path + draft.text} dataUrl={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(draft.text)}`} path={draft.path} />
            : isMarkdownPreviewFile(draft.path)
            ? <MarkdownFilePreview api={api} sessionId={sessionId} tabId={tabId} path={draft.path} text={draft.text} onOpen={openPath} />
            : <HtmlFilePreview api={api} sessionId={sessionId} tabId={tabId} path={draft.path} text={draft.text} />
          : <FileCodeEditor path={draft.path} text={draft.text} request={document.request} onChange={document.setText} onSave={() => void document.save()} />)}
      </section>
    </>}
    {pending && <div ref={dialog} className="file-dirty-confirm" role="alertdialog" aria-modal="true" aria-label="Unsaved changes">
      <p>{imageDirty ? 'Save the image using its controls before continuing, or discard its changes.' : `Save changes to ${draft ? fileName(draft.path) : 'this file'}?`}</p>
      <div>
        <button autoFocus disabled={document.saving} onClick={() => setPending(null)}>Cancel</button>
        <button disabled={document.saving} onClick={() => {
          if (file?.kind === 'image') { discardImageEditorDraft(file, imageDraftScope); setImageEditing(false); updateImageDirty(false); }
          document.discard(); const action = pending; setPending(null); action();
        }}>Discard changes</button>
        {!imageDirty && <button disabled={document.saving} onClick={() => { void document.save().then(saved => { if (saved) { const action = pending; setPending(null); action(); } }); }}>Save and continue</button>}
      </div>
    </div>}
  </aside>;
}
