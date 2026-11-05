import { session, type BrowserWindow } from 'electron';
import { installFilePreviewProtocol } from './file-preview-protocol';
import { FILE_PREVIEW_CSP, FILE_PREVIEW_SCHEME } from '../shared/file-preview-url';

export function installDesktopPolicy(): void {
  const csp = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'none'; base-uri 'none'; form-action 'none'; frame-src shellfox-preview:; object-src 'none'";
  installFilePreviewProtocol();
  session.defaultSession.webRequest.onHeadersReceived((details, callback) => callback({ responseHeaders: { ...details.responseHeaders, 'Content-Security-Policy': [details.url.startsWith(`${FILE_PREVIEW_SCHEME}:`) ? FILE_PREVIEW_CSP : csp] } }));
  session.defaultSession.setPermissionRequestHandler((_contents, _permission, callback) => callback(false));
  session.defaultSession.setPermissionCheckHandler(() => false);
}
export function secureDesktopWindow(window: BrowserWindow): void {
  window.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  window.webContents.on('will-navigate', event => event.preventDefault());
  window.webContents.on('will-redirect', event => event.preventDefault());
  window.webContents.on('will-attach-webview', event => event.preventDefault());
}
