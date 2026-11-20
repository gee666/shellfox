import { session, type BrowserWindow, type WebFrameMain } from 'electron';
import { installFilePreviewProtocol } from './file-preview-protocol';
import { FILE_PREVIEW_CSP, FILE_PREVIEW_SCHEME } from '../shared/file-preview-url';

export function installDesktopPolicy(): void {
  const csp = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'none'; base-uri 'none'; form-action 'none'; frame-src shellfox-preview:; object-src 'none'";
  installFilePreviewProtocol();
  session.defaultSession.webRequest.onHeadersReceived((details, callback) => callback({ responseHeaders: { ...details.responseHeaders, 'Content-Security-Policy': [details.url.startsWith(`${FILE_PREVIEW_SCHEME}:`) ? FILE_PREVIEW_CSP : csp] } }));
  session.defaultSession.setPermissionRequestHandler((_contents, _permission, callback) => callback(false));
  session.defaultSession.setPermissionCheckHandler(() => false);
}
type FrameAddress = Pick<WebFrameMain, 'processId' | 'routingId'>;

export function permitPreviewNavigation(url: string, initiator: FrameAddress | null | undefined, renderer: FrameAddress): boolean {
  if (!url.startsWith(`${FILE_PREVIEW_SCHEME}://document/?html=`)) return false;
  return !initiator || (initiator.processId === renderer.processId && initiator.routingId === renderer.routingId);
}

export function secureDesktopWindow(window: BrowserWindow): void {
  const { webContents } = window;
  webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  webContents.on('will-navigate', event => event.preventDefault());
  // The renderer may load previews, but an existing preview cannot replace
  // itself with a different document. Keep the frame-src CSP in place.
  webContents.on('will-frame-navigate', event => {
    if (!event.isMainFrame && !permitPreviewNavigation(event.url, event.initiator, webContents.mainFrame)) {
      event.preventDefault();
    }
  });
  webContents.on('will-redirect', event => event.preventDefault());
  webContents.on('will-attach-webview', event => event.preventDefault());
}
