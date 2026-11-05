// The opaque iframe pattern is adapted from T3 Code's BrowserDocumentFrame.tsx.
// Copyright (c) 2026 T3 Tools Inc. MIT license: ./T3CODE-LICENSE.txt.
import { useEffect, useState } from 'react';
import ReactMarkdown, { defaultUrlTransform } from 'react-markdown';
import remarkGfm from 'remark-gfm';
import type { ManagerApi } from '../../shared/contracts';
import { prepareFileHtml } from '../../shared/file-html-preview';
import { desktopPreviewUrl } from '../../shared/file-preview-url';
import { fileName, maskMarkdownFrontmatter } from './file-preview-utils';

type FileApi = NonNullable<ManagerApi['files']>;
interface PreviewProps { api: ManagerApi; sessionId?: string; tabId?: string; path: string; text: string }
const HTML_CSP = "default-src 'none'; script-src 'unsafe-inline' data: blob:; style-src 'unsafe-inline' data: blob:; img-src data: blob:; font-src data: blob:; media-src data: blob:; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";

async function readAsset(api: FileApi | undefined, target: { sessionId?: string; tabId?: string; path: string }, assetPath: string) {
  if (!api) throw new Error('File assets are not supported by this host.');
  const result = await api.readAsset({ ...target, assetPath });
  if (!result.ok) throw new Error(result.error.message);
  return result.value;
}

export function HtmlFilePreview({ api, sessionId, tabId, path, text }: PreviewProps) {
  const [previewUrl, setPreviewUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [warnings, setWarnings] = useState<string[]>([]);
  useEffect(() => {
    let active = true;
    const missing: string[] = [];
    setPreviewUrl(null); setError(null); setWarnings([]);
    void prepareFileHtml(text, async assetPath => {
      try { return (await readAsset(api.files, { sessionId, tabId, path }, assetPath)).dataUrl; }
      catch (cause) { missing.push(`${assetPath}: ${cause instanceof Error ? cause.message : 'Unavailable'}`); return null; }
    }).then(prepared => {
      if (!active) return;
      // This is the first policy in the document. File-supplied policies cannot loosen it.
      setWarnings(missing);
      setPreviewUrl(desktopPreviewUrl(`<!doctype html><meta http-equiv="Content-Security-Policy" content="${HTML_CSP}"><meta name="referrer" content="no-referrer">${prepared}`));
    }).catch((cause: unknown) => { if (active) setError(cause instanceof Error ? cause.message : 'HTML preview failed.'); });
    return () => { active = false; };
  }, [api, sessionId, tabId, path, text]);
  if (error) return <p className="file-feedback" role="alert">{error}</p>;
  if (previewUrl === null) return <p className="file-feedback" role="status">Preparing HTML preview…</p>;
  // Never allow-same-origin. A file must not access the application or its preload bridge.
  return <>
    {warnings.length > 0 && <details className="file-preview-warnings"><summary>{warnings.length} unavailable preview assets</summary><ul>{warnings.map((warning, index) => <li key={index}>{warning}</li>)}</ul></details>}
    <iframe className="file-html-preview" title="HTML preview" sandbox="allow-scripts" src={previewUrl} referrerPolicy="no-referrer" />
  </>;
}

function MarkdownImage({ api, sessionId, tabId, path, source, alt }: Omit<PreviewProps, 'text'> & { source: string; alt: string }) {
  const [url, setUrl] = useState<string | null>(null);
  const [error, setError] = useState(false);
  useEffect(() => {
    let active = true;
    setUrl(null); setError(false);
    void readAsset(api.files, { sessionId, tabId, path }, source).then(asset => {
      if (active && asset.mimeType.startsWith('image/')) setUrl(asset.dataUrl);
      else if (active) setError(true);
    }).catch(() => { if (active) setError(true); });
    return () => { active = false; };
  }, [api, sessionId, tabId, path, source]);
  if (error) return <span className="file-image-error" role="status">Image unavailable: {alt || source}</span>;
  return url ? <img src={url} alt={alt} /> : <span role="status">Loading image…</span>;
}

export function MarkdownFilePreview(props: PreviewProps & { onOpen: (path: string) => void }) {
  const [error, setError] = useState<string | null>(null);
  return <article className="file-markdown-preview" aria-label="Markdown preview">
    {error && <p role="alert">{error}</p>}
    <ReactMarkdown remarkPlugins={[remarkGfm]} urlTransform={(url, key) => key === 'src' ? url : defaultUrlTransform(url)} components={{
      img: ({ src, alt }) => <MarkdownImage {...props} source={typeof src === 'string' ? src : ''} alt={alt ?? ''} />,
      a: ({ href, children }) => <a href={href} onClick={event => {
        event.preventDefault();
        if (!href) return;
        if (href.startsWith('#')) {
          event.currentTarget.closest('article')?.querySelector(`[id="${CSS.escape(href.slice(1))}"]`)?.scrollIntoView();
          return;
        }
        if (/^https?:\/\//i.test(href)) {
          if (!props.api.openExternal) { setError('External links are not available.'); return; }
          void props.api.openExternal({ url: href }).then(result => { if (!result.ok) setError(result.error.message); }).catch(() => setError('Could not open link.'));
          return;
        }
        if (/^[a-z][a-z\d+.-]*:/i.test(href)) return;
        const separator = Math.max(props.path.lastIndexOf('/'), props.path.lastIndexOf('\\'));
        const folder = props.path.slice(0, separator + 1);
        try { props.onOpen(folder + decodeURIComponent(href.split('#')[0])); }
        catch { setError('Invalid file link.'); }
      }}>{children}</a>,
    }}>{maskMarkdownFrontmatter(props.text)}</ReactMarkdown>
  </article>;
}

export function FileImagePreview({ dataUrl, path }: { dataUrl: string; path: string }) {
  const [failed, setFailed] = useState(false);
  return <div className="file-image-preview">{failed ? <p role="alert">Could not display image.</p>
    : <img src={dataUrl} alt={fileName(path)} onError={() => setFailed(true)} />}</div>;
}
