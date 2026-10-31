import { ipcMain, type BrowserWindow } from 'electron';
import { randomUUID } from 'node:crypto';
import type { RemoteViewPort } from './view';
import { failure } from '../../shared/contracts';
import { remoteEnvelopeSchema, remoteRequestSchemas, remoteResponseSchemas } from '../../shared/remote-schemas';
import { isTrustedSender } from '../ipc';
import { sendToRenderer } from '../renderer-channel';
import type { RemoteService } from './service';
export function installRemoteIpc(window: BrowserWindow, rendererUrl: string, service: RemoteService | null): () => void {
  let view: RemoteViewPort | undefined;
  let epoch = 0;
  const release = () => { epoch++; const previous = view; view = undefined; void previous?.dispose(); };
  const getView = () => {
    if (!service) throw new Error('Remote access unavailable.');
    if (!view) {
      const current = epoch;
      view = service.createView(randomUUID());
      view.subscribe(event => { if (current === epoch) sendToRenderer(window, rendererUrl, 'remote:event', event); });
    }
    return view;
  };
  ipcMain.handle('remote:request', async (event, raw: unknown) => {
    if (!isTrustedSender(event, window, rendererUrl)) return failure('AUTH_FAILED', 'Untrusted remote IPC sender.');
    if (!service) return failure('UNSUPPORTED', 'Remote access requires embedded terminals.');
    const envelope = remoteEnvelopeSchema.safeParse(raw);
    if (!envelope.success) return failure('VALIDATION', 'Invalid remote envelope.');
    const { method, payload } = envelope.data;
    if (!remoteRequestSchemas[method].safeParse(payload).success) return failure('VALIDATION', 'Invalid remote payload.');
    try {
      let result: unknown;
      switch (method) {
        case 'getState': result = service.getState(); break;
        case 'configure': result = await service.configure(remoteRequestSchemas.configure.parse(payload)); break;
        case 'login': result = await service.login(remoteRequestSchemas.login.parse(payload)); break;
        case 'connect': result = await service.connect(remoteRequestSchemas.connect.parse(payload)); break;
        case 'disconnect': result = await service.disconnect(remoteRequestSchemas.disconnect.parse(payload)); break;
        case 'approve': result = await service.approve(remoteRequestSchemas.approve.parse(payload)); break;
        case 'reject': result = await service.reject(remoteRequestSchemas.reject.parse(payload)); break;
        case 'revoke': result = await service.revoke(remoteRequestSchemas.revoke.parse(payload)); break;
        case 'forget': result = await service.forget(remoteRequestSchemas.forget.parse(payload)); break;
        case 'request': return await getView().request(payload);
      }
      const parsed = remoteResponseSchemas[method].safeParse(result);
      return parsed.success ? parsed.data : failure('INTERNAL', 'Invalid remote state.');
    } catch { return failure('INTERNAL', 'Remote operation failed.', true); }
  });
  const unsubscribe = service?.subscribe(event => {
    if (event.type === 'state') sendToRenderer(window, rendererUrl, 'remote:event', event);
  });
  window.webContents.on?.('render-process-gone', release);
  window.webContents.on?.('did-start-navigation', release);
  window.webContents.on?.('destroyed', release);
  return () => { release(); unsubscribe?.(); window.webContents.removeListener?.('render-process-gone', release); window.webContents.removeListener?.('did-start-navigation', release); window.webContents.removeListener?.('destroyed', release); ipcMain.removeHandler('remote:request'); };
}
