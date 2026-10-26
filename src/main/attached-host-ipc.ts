import { ipcMain, type BrowserWindow } from 'electron';
import type { HostClient } from '../host/client';
import { failure, type ManagerSnapshot } from '../shared/contracts';
import { requestEnvelopeSchema, requestSchemas, responseSchemas } from '../shared/schemas';
import { remoteEnvelopeSchema, remoteRequestSchemas, remoteResponseSchemas } from '../shared/remote-schemas';
import { desktopRequest } from './desktop-requests';
import { isTrustedSender } from './ipc';
import type { UpdateSource } from './update/self-updater';

/** This window is a client. It never opens the host database or owns its PTYs. */
export function installAttachedHostIpc(window: BrowserWindow, rendererUrl: string, client: HostClient, ready: () => void, updates?: UpdateSource): () => void {
  let announced = false;
  let snapshot: ManagerSnapshot | undefined;
  const trusted = (event: Parameters<typeof isTrustedSender>[0]) => isTrustedSender(event, window, rendererUrl);
  ipcMain.handle('manager:request', async (event, raw: unknown) => {
    if (!trusted(event)) return failure('AUTH_FAILED', 'Untrusted application renderer.');
    const envelope = requestEnvelopeSchema.safeParse(raw);
    if (!envelope.success) return failure('VALIDATION', 'Invalid request envelope.');
    const { method, payload } = envelope.data;
    const input = requestSchemas[method].safeParse(payload);
    if (!input.success) return failure('VALIDATION', 'Invalid request payload.');
    try {
      let result = await desktopRequest(method, payload, { window, updates, sessionDirectory: async id => {
        const snapshot = await client.service.getSnapshot();
        return snapshot.ok ? snapshot.value.sessions.find(session => session.id === id)?.cwd ?? null : null;
      } });
      if (result === undefined) result = await client.request(method, input.data);
      const response = responseSchemas[method].safeParse(result);
      if (method === 'getSnapshot') {
        const current = responseSchemas.getSnapshot.safeParse(result);
        if (current.success && current.data.ok) {
          snapshot = current.data.value;
          if (!announced) { announced = true; ready(); }
        }
      }
      return response.success ? response.data : failure('INTERNAL', 'Invalid host response.');
    } catch { return failure('NATIVE_UNAVAILABLE', 'The host connection was lost. No uncertain operation was retried.', true); }
  });
  ipcMain.handle('remote:request', async (event, raw: unknown) => {
    if (!trusted(event)) return failure('AUTH_FAILED', 'Untrusted application renderer.');
    const envelope = remoteEnvelopeSchema.safeParse(raw);
    if (!envelope.success) return failure('VALIDATION', 'Invalid remote envelope.');
    const { method, payload } = envelope.data;
    if (!remoteRequestSchemas[method].safeParse(payload).success) return failure('VALIDATION', 'Invalid remote payload.');
    try {
      let result: unknown;
      switch (method) {
        case 'getState': result = await client.remote.getState(); break;
        case 'configure': result = await client.remote.configure(remoteRequestSchemas.configure.parse(payload)); break;
        case 'login': result = await client.remote.login(remoteRequestSchemas.login.parse(payload)); break;
        case 'connect': result = await client.remote.connect(remoteRequestSchemas.connect.parse(payload)); break;
        case 'disconnect': result = await client.remote.disconnect(remoteRequestSchemas.disconnect.parse(payload)); break;
        case 'approve': result = await client.remote.approve(remoteRequestSchemas.approve.parse(payload)); break;
        case 'reject': result = await client.remote.reject(remoteRequestSchemas.reject.parse(payload)); break;
        case 'revoke': result = await client.remote.revoke(remoteRequestSchemas.revoke.parse(payload)); break;
        case 'forget': result = await client.remote.forget(remoteRequestSchemas.forget.parse(payload)); break;
        case 'request': {
          const input = remoteRequestSchemas.request.parse(payload);
          return responseSchemas[input.method].parse(await client.request('remote.request', input));
        }
      }
      const response = remoteResponseSchemas[method].safeParse(result);
      return response.success ? response.data : failure('INTERNAL', 'Invalid remote host response.');
    } catch { return failure('NATIVE_UNAVAILABLE', 'The host connection was lost.', true); }
  });
  const send = (channel: string, value: unknown) => {
    if (!window.isDestroyed() && window.webContents.mainFrame.url === rendererUrl) window.webContents.send(channel, value);
  };
  const disconnected = client.onDisconnect(() => {
    // An old display must not accept input after its event stream has stopped.
    for (const session of snapshot?.sessions ?? []) for (const tab of session.tabs) {
      if (tab.generation && tab.lifecycle === 'open') send('manager:terminal', { type: 'error', tabId: tab.id, generation: tab.generation,
        error: { code: 'NATIVE_UNAVAILABLE', message: 'Host disconnected. Reopen Shellfox to reconnect.', retryable: true } });
    }
  });
  const cleanup = [disconnected, client.service.subscribe(event => send('manager:changed', event)), client.service.subscribeTerminal(event => send('manager:terminal', event)), client.remote.subscribe(event => send('remote:event', event)), client.service.subscribeKeepAwake?.(event => send('manager:keep-awake', event))];
  return () => { cleanup.forEach(unsubscribe => unsubscribe?.()); ipcMain.removeHandler('manager:request'); ipcMain.removeHandler('remote:request'); };
}
