import { ipcMain, type BrowserWindow } from 'electron';
import type { HostClient } from '../host/client';
import { failure, type TerminalEvent } from '../shared/contracts';
import { requestEnvelopeSchema, requestSchemas, responseSchemas } from '../shared/schemas';
import { remoteEnvelopeSchema, remoteRequestSchemas, remoteResponseSchemas } from '../shared/remote-schemas';
import { desktopRequest } from './desktop-requests';
import { isTrustedSender } from './ipc';
import type { UpdateSource } from './update/self-updater';

/** Every renderer document gets its own host view, separate from desktop metadata. */
export function installAttachedHostIpc(window: BrowserWindow, rendererUrl: string, client: HostClient, ready: () => void, updates?: UpdateSource): () => Promise<void> {
  let announced = false, stopped = false, epoch = 0;
  let document: Promise<HostClient> | undefined;
  const closing = new Set<Promise<void>>();
  const attachments = new Map<string, { generation: string; controlEpoch: number }>();
  const send = (channel: string, value: unknown) => {
    if (!window.isDestroyed() && window.webContents.mainFrame.url === rendererUrl) window.webContents.send(channel, value);
  };
  function terminal(event: TerminalEvent): void {
    if (event.type === 'control') attachments.set(event.tabId, { generation: event.generation, controlEpoch: event.controlEpoch });
    send('manager:terminal', event);
  }
  function view(): Promise<HostClient> {
    if (stopped) return Promise.reject(new Error('Renderer view closed.'));
    if (document) return document;
    const current = epoch;
    const pending = client.fork(); document = pending;
    void pending.then(child => {
      if (stopped || current !== epoch) return;
      child.service.subscribeTerminal(event => { if (current === epoch) terminal(event); });
      child.remote.subscribe(event => { if (current === epoch && event.type !== 'state') send('remote:event', event); });
      child.onDisconnect(() => {
        if (current !== epoch) return;
        for (const [tabId, identity] of attachments) terminal({ type: 'error', tabId, ...identity, error: { code: 'NATIVE_UNAVAILABLE', message: 'Host disconnected. Reopen Shellfox to reconnect.', retryable: true } });
      });
    }).catch(() => {});
    return pending;
  }
  const releaseView = () => {
    epoch++; attachments.clear(); const old = document; document = undefined;
    if (old) { const task = old.then(child => child.close()).catch(() => {}); closing.add(task); void task.finally(() => closing.delete(task)); }
  };
  const trusted = (event: Parameters<typeof isTrustedSender>[0]) => isTrustedSender(event, window, rendererUrl);
  ipcMain.handle('manager:request', async (event, raw: unknown) => {
    if (!trusted(event)) return failure('AUTH_FAILED', 'Untrusted application renderer.');
    const envelope = requestEnvelopeSchema.safeParse(raw);
    if (!envelope.success) return failure('VALIDATION', 'Invalid request envelope.');
    const { method, payload } = envelope.data;
    const input = requestSchemas[method].safeParse(payload);
    if (!input.success) return failure('VALIDATION', 'Invalid request payload.');
    const current = epoch;
    try {
      const scoped = ['attachTerminal', 'takeTerminalControl', 'writeTerminal', 'resizeTerminal', 'acknowledgeTerminal', 'detachTerminal', 'setVisibleSessions'].includes(method);
      const target = scoped ? await view() : client;
      if (scoped && current !== epoch) return failure('TARGET_LOST', 'Renderer view changed.');
      let result = await desktopRequest(method, payload, { window, updates, sessionDirectory: async id => {
        const snapshot = await client.service.getSnapshot();
        return snapshot.ok ? snapshot.value.sessions.find(session => session.id === id)?.cwd ?? null : null;
      } });
      if (result === undefined) result = await target.request(method, input.data);
      if (method === 'attachTerminal' && current === epoch) {
        const attached = responseSchemas.attachTerminal.safeParse(result);
        if (attached.success && attached.data.ok) attachments.set(attached.data.value.tabId, { generation: attached.data.value.generation, controlEpoch: attached.data.value.controlEpoch });
      }
      if (method === 'detachTerminal') attachments.delete(requestSchemas.detachTerminal.parse(payload).tabId);
      if (method === 'getSnapshot' && !announced) {
        const snapshot = responseSchemas.getSnapshot.safeParse(result);
        if (snapshot.success && snapshot.data.ok) { announced = true; ready(); }
      }
      const response = responseSchemas[method].safeParse(result);
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
          const current = epoch, target = await view();
          if (current !== epoch) return failure('TARGET_LOST', 'Renderer view changed.');
          const input = remoteRequestSchemas.request.parse(payload);
          return responseSchemas[input.method].parse(await target.request('remote.request', input));
        }
      }
      const response = remoteResponseSchemas[method].safeParse(result);
      return response.success ? response.data : failure('INTERNAL', 'Invalid remote host response.');
    } catch { return failure('NATIVE_UNAVAILABLE', 'The host connection was lost.', true); }
  });
  const cleanup = [client.service.subscribe(event => send('manager:changed', event)), client.remote.subscribe(event => { if (event.type === 'state') send('remote:event', event); }), client.service.subscribeKeepAwake?.(event => send('manager:keep-awake', event))];
  window.webContents.on?.('render-process-gone', releaseView);
  window.webContents.on?.('did-start-navigation', releaseView);
  window.webContents.on?.('destroyed', releaseView);
  return async () => {
    stopped = true; releaseView();
    window.webContents.removeListener?.('render-process-gone', releaseView);
    window.webContents.removeListener?.('did-start-navigation', releaseView);
    window.webContents.removeListener?.('destroyed', releaseView);
    cleanup.forEach(unsubscribe => unsubscribe?.()); ipcMain.removeHandler('manager:request'); ipcMain.removeHandler('remote:request');
    await Promise.allSettled(closing);
  };
}
