import { protocol } from 'electron';
import { FILE_PREVIEW_CSP, FILE_PREVIEW_SCHEME, FILE_PREVIEW_URL_LIMIT } from '../shared/file-preview-url';

/** Register before Electron is ready. This scheme serves document bytes only, never disk paths. */
export function registerFilePreviewScheme(): void {
  protocol.registerSchemesAsPrivileged([{ scheme: FILE_PREVIEW_SCHEME, privileges: { standard: true, secure: true } }]);
}
export function previewDocumentResponse(request: { url: string; method: string }): Response {
  try {
    if (request.method !== 'GET' || request.url.length > FILE_PREVIEW_URL_LIMIT) return new Response('Preview request rejected.', { status: 400 });
    const url = new URL(request.url);
    if (url.protocol !== `${FILE_PREVIEW_SCHEME}:` || url.hostname !== 'document' || url.pathname !== '/' || url.username || url.password || url.port) return new Response('Not found.', { status: 404 });
    const document = url.searchParams.get('html');
    if (document === null) return new Response('Preview document missing.', { status: 400 });
    return new Response(document, { headers: {
      'Content-Type': 'text/html; charset=utf-8',
      'Content-Security-Policy': FILE_PREVIEW_CSP,
      'X-Content-Type-Options': 'nosniff',
      'Referrer-Policy': 'no-referrer',
      'Cache-Control': 'no-store',
    } });
  } catch { return new Response('Preview request rejected.', { status: 400 }); }
}
export function installFilePreviewProtocol(): void {
  protocol.handle(FILE_PREVIEW_SCHEME, request => previewDocumentResponse(request));
}
