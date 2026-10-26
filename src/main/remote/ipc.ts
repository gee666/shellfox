import { ipcMain, type BrowserWindow } from 'electron';
import { failure } from '../../shared/contracts';
import { remoteEnvelopeSchema, remoteRequestSchemas, remoteResponseSchemas } from '../../shared/remote-schemas';
import { isTrustedSender } from '../ipc';
import type { RemoteService } from './service';
export function installRemoteIpc(window: BrowserWindow, rendererUrl: string, service: RemoteService | null): () => void {
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
        case 'request': return await service.request(payload);
      }
      const parsed = remoteResponseSchemas[method].safeParse(result);
      return parsed.success ? parsed.data : failure('INTERNAL', 'Invalid remote state.');
    } catch { return failure('INTERNAL', 'Remote operation failed.', true); }
  });
  const unsubscribe = service?.subscribe(event => {
    if (!window.isDestroyed() && window.webContents.mainFrame.url === rendererUrl) window.webContents.send('remote:event', event);
  });
  return () => { unsubscribe?.(); ipcMain.removeHandler('remote:request'); };
}
