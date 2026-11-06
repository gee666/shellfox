import { useEffect, useState } from 'react';
import { prepareFileHtml } from '../../src/shared/file-html-preview';
import type { FileLocation } from '../../src/shared/file-contracts';
import type { BrowserFile, FileApi } from './file-browser-types';
import { renderFileMarkdown, textPreviewKind } from './file-preview-document';

export function useFileHtmlPreview(api: FileApi, context: Omit<FileLocation, 'path'>, file: BrowserFile | null, draft: string, available: boolean, report: (message: string) => void) {
  const [html, setHtml] = useState<string>();
  const [loading, setLoading] = useState(false);
  useEffect(() => {
    const kind = file?.kind === 'text' ? textPreviewKind(file.path, file.mimeType) : null;
    if (file?.kind !== 'text' || kind !== 'html' && kind !== 'markdown') { setHtml(undefined); setLoading(false); return; }
    let current = true;
    setLoading(true); setHtml(undefined);
    const timer = setTimeout(() => {
      const source = kind === 'markdown' ? renderFileMarkdown(draft) : draft;
      void prepareFileHtml(source, async assetPath => {
        if (!current) throw new Error('Preview changed.');
        if (!available) return null;
        const result = await api.readAsset({ ...context, path: file.path, assetPath });
        return result.ok ? result.value.dataUrl : null;
      }).then(value => { if (current) setHtml(value); })
        .catch(cause => { if (current) report(cause instanceof Error ? cause.message : 'Unable to prepare file preview.'); })
        .finally(() => { if (current) setLoading(false); });
    }, 250);
    return () => { current = false; clearTimeout(timer); };
  }, [api, context.sessionId, context.tabId, file?.path, file?.revision, draft, available, report]);
  return { html, loading };
}
