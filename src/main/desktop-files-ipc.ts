import { app, dialog, ipcMain, type BrowserWindow } from 'electron';
import path from 'node:path';
import { failure } from '../shared/contracts';
import { DESKTOP_FILES_CHANNEL, desktopFileEnvelopeSchema, desktopFileRequestSchemas, desktopFileResponseSchemas } from '../shared/desktop-file-schemas';
import { isTrustedSender } from './ipc';
import { DesktopFileHandles } from './desktop-file-handles';

function suggestedName(name: string): string {
  const safe = name.replace(/[<>:"/\\|?*\x00-\x1f\x7f]/g, '_').replace(/[. ]+$/, '');
  return !safe || /^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(safe) ? `download-${safe || 'file'}` : safe;
}

/** This channel is always local, including desktops attached to a headless or remote host. */
export function installDesktopFilesIpc(window: BrowserWindow, rendererUrl: string): () => Promise<void> {
  let stopped = false;
  const create = () => new DesktopFileHandles({
    upload: async () => {
      const chosen = await dialog.showOpenDialog(window, { title: 'Upload file', properties: ['openFile'] });
      return chosen.canceled ? null : chosen.filePaths[0] ?? null;
    },
    download: async name => {
      const chosen = await dialog.showSaveDialog(window, {
        title: 'Download file', defaultPath: path.join(app.getPath('downloads'), suggestedName(name)),
        properties: ['showOverwriteConfirmation', 'createDirectory'],
      });
      return chosen.canceled ? null : chosen.filePath ?? null;
    },
  });
  let current = create();
  const closing = new Set<Promise<void>>();
  const release = () => {
    const pending = current.dispose(); closing.add(pending);
    void pending.finally(() => closing.delete(pending));
  };
  const reset = () => { if (!stopped) { release(); current = create(); } };
  const navigate = (_event: unknown, _url: string, inPlace: boolean, mainFrame: boolean) => { if (mainFrame && !inPlace) reset(); };
  ipcMain.handle(DESKTOP_FILES_CHANNEL, async (event, raw: unknown) => {
    if (stopped || !isTrustedSender(event, window, rendererUrl)) return failure('AUTH_FAILED', 'Untrusted local file request.');
    const envelope = desktopFileEnvelopeSchema.safeParse(raw);
    if (!envelope.success) return failure('VALIDATION', 'Invalid local file request.');
    const { method, payload } = envelope.data;
    if (!desktopFileRequestSchemas[method].safeParse(payload).success) return failure('VALIDATION', 'Invalid local file payload.');
    const owner = current;
    try {
      let result: unknown;
      switch (method) {
        case 'pickUpload': result = await owner.pickUpload(); break;
        case 'readUpload': result = await owner.readUpload(desktopFileRequestSchemas.readUpload.parse(payload)); break;
        case 'releaseUpload': result = await owner.releaseUpload(desktopFileRequestSchemas.releaseUpload.parse(payload)); break;
        case 'beginDownload': result = await owner.beginDownload(desktopFileRequestSchemas.beginDownload.parse(payload)); break;
        case 'writeDownload': result = await owner.writeDownload(desktopFileRequestSchemas.writeDownload.parse(payload)); break;
        case 'finishDownload': result = await owner.finishDownload(desktopFileRequestSchemas.finishDownload.parse(payload)); break;
        case 'cancelDownload': result = await owner.cancelDownload(desktopFileRequestSchemas.cancelDownload.parse(payload)); break;
      }
      if (stopped || owner !== current || !isTrustedSender(event, window, rendererUrl)) return failure('AUTH_FAILED', 'The local file owner document changed.');
      const parsed = desktopFileResponseSchemas[method].safeParse(result);
      return parsed.success ? parsed.data : failure('INTERNAL', 'Invalid local file response.');
    } catch { return failure('STORAGE_FAILED', 'Local file operation failed.', true); }
  });
  const dispose = async () => {
    if (!stopped) {
      stopped = true; release();
      ipcMain.removeHandler(DESKTOP_FILES_CHANNEL);
      window.webContents.removeListener('did-start-navigation', navigate);
      window.webContents.removeListener('render-process-gone', reset);
      window.webContents.removeListener('destroyed', destroyed);
    }
    await Promise.allSettled([...closing]);
  };
  const destroyed = () => { void dispose(); };
  window.webContents.on('did-start-navigation', navigate);
  window.webContents.on('render-process-gone', reset);
  window.webContents.on('destroyed', destroyed);
  return dispose;
}
