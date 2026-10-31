import { ipcMain, type BrowserWindow, type IpcMainInvokeEvent } from 'electron';
import { failure, success } from '../shared/contracts';
import { requestEnvelopeSchema, requestSchemas, responseSchemas } from '../shared/schemas';
import type { SessionService } from './service';
import type { EmbeddedSessionService } from './terminal/service';
import { desktopRequest } from './desktop-requests';
import { getHomeDirectory, resolveDirectory, createDirectory, completeDirectory } from './directory-prompt';
import { SshProfileStore } from './ssh/storage';
import { readPuttySessions } from './ssh/putty';
import type { UpdateSource } from './update/self-updater';
import type { KeepAwake } from './keep-awake';
import { rendererShowing, sendToRenderer } from './renderer-channel';

export function isTrustedSender(event: IpcMainInvokeEvent, window: BrowserWindow, rendererUrl: string): boolean {
  return rendererShowing(window, rendererUrl) && event.sender === window.webContents && !!event.senderFrame &&
    event.senderFrame === window.webContents.mainFrame && event.senderFrame.url === rendererUrl;
}
export function installIpc(window: BrowserWindow, rendererUrl: string, service: SessionService | EmbeddedSessionService, rendererReady?: () => void, updates?: UpdateSource, ssh?: SshProfileStore, keepAwake?: KeepAwake, assertOwnership?: () => void): () => void {
  let announcedReady = false;
  const embedded = 'attachTerminal' in service ? service : null;
  const sendTerminal = (event: import('../shared/contracts').TerminalEvent) => {
    sendToRenderer(window, rendererUrl, 'manager:terminal', event);
  };
  let stopped = false;
  let delivery = embedded?.createTerminalView(sendTerminal) ?? null;
  ipcMain.handle('manager:request', async (event, raw: unknown) => {
    if (!isTrustedSender(event, window, rendererUrl)) return failure('AUTH_FAILED', 'Request did not come from the application renderer');
    try { assertOwnership?.(); } catch { return failure('NATIVE_UNAVAILABLE', 'Host ownership was lost. Shellfox is shutting down.'); }
    const envelope = requestEnvelopeSchema.safeParse(raw);
    if (!envelope.success) return failure('VALIDATION', 'Invalid request envelope');
    const { method, payload } = envelope.data;
    const validated = requestSchemas[method].safeParse(payload);
    if (!validated.success) return failure('VALIDATION', 'Invalid request payload');
    if ((method === 'activateSession' || method === 'focusSession') && window.isFocused()) {
      const backend = service.backend;
      if ('authorizeForegroundFocus' in backend && typeof backend.authorizeForegroundFocus === 'function') backend.authorizeForegroundFocus();
    }
    try {
      let result = await desktopRequest(method, payload, { window, updates, sessionDirectory: id => service.repository.session(id)?.cwd ?? null });
      // Keep methods explicit. There is no property lookup into the service.
      if (result === undefined) switch (method) {
        case 'listSshProfiles': result = ssh ? await ssh.list() : failure('UNSUPPORTED', 'SSH storage is unavailable.'); break;
        case 'saveSshProfile': result = ssh ? await ssh.save(requestSchemas.saveSshProfile.parse(payload)) : failure('UNSUPPORTED', 'SSH storage is unavailable.'); break;
        case 'deleteSshProfile': result = ssh ? await ssh.delete(requestSchemas.deleteSshProfile.parse(payload).id) : failure('UNSUPPORTED', 'SSH storage is unavailable.'); break;
        case 'importPuttySessions': {
          if(!ssh){result=failure('UNSUPPORTED','SSH storage is unavailable.');break;}
          try {result=await ssh.import(await readPuttySessions());}
          catch(error){result=failure('STORAGE_FAILED',('PuTTY import failed: '+(error as Error).message).replace(/[\x00-\x1f\x7f]/g,' ').slice(0,1000),true);}
          break;
        }
        case 'getSnapshot':
          // Registry/PATH status is metadata, not terminal readiness. Return
          // runtime state now; changed metadata publishes its own notification.
          void service.refreshIntegrations?.().catch(() => {});
          result = service.getSnapshot();
          // The renderer subscribes before its first snapshot request. Load completion alone is not enough.
          if (!announcedReady) { announcedReady = true; rendererReady?.(); }
          break;
        case 'getSessionStats': result = embedded ? await embedded.getSessionStats(requestSchemas.getSessionStats.parse(payload)) : failure('UNSUPPORTED', 'Process stats require embedded terminals.'); break;
        case 'setVisibleSessions': result = embedded ? embedded.setVisibleSessions(requestSchemas.setVisibleSessions.parse(payload)) : success({ configured: true }); break;
        case 'getTerminalProfiles': result = embedded ? embedded.getTerminalProfiles() : failure('UNSUPPORTED', 'Embedded terminals are unavailable in this backend.'); break;
        case 'attachTerminal': result = delivery ? delivery.attach(requestSchemas.attachTerminal.parse(payload)) : failure('UNSUPPORTED', 'Embedded terminals are unavailable in this backend.'); break;
        case 'takeTerminalControl': result = delivery ? delivery.take(requestSchemas.takeTerminalControl.parse(payload)) : failure('UNSUPPORTED', 'Embedded terminals are unavailable in this backend.'); break;
        case 'writeTerminal': result = delivery ? delivery.write(requestSchemas.writeTerminal.parse(payload)) : failure('UNSUPPORTED', 'Embedded terminals are unavailable in this backend.'); break;
        case 'resizeTerminal': result = delivery ? delivery.resize(requestSchemas.resizeTerminal.parse(payload)) : failure('UNSUPPORTED', 'Embedded terminals are unavailable in this backend.'); break;
        case 'closeTab': result = embedded ? await embedded.closeTab(requestSchemas.closeTab.parse(payload)) : failure('UNSUPPORTED', 'External processes cannot be closed by embedded terminal controls.'); break;
        case 'acknowledgeTerminal': result = delivery ? delivery.acknowledge(requestSchemas.acknowledgeTerminal.parse(payload)) : failure('UNSUPPORTED', 'Embedded terminals are unavailable in this backend.'); break;
        case 'detachTerminal': {
          const input = requestSchemas.detachTerminal.parse(payload);
          result = delivery ? delivery.detach(input) : failure('UNSUPPORTED', 'Embedded terminals are unavailable in this backend.');
          break;
        }
        case 'activateSession': result = embedded ? await embedded.activateSession(requestSchemas.activateSession.parse(payload), delivery ?? undefined) : await service.activateSession(requestSchemas.activateSession.parse(payload)); break;
        case 'refreshSessionMembership': result = await service.refreshSessionMembership(requestSchemas.refreshSessionMembership.parse(payload)); break;
        case 'prepareSessionRegistration': result = await service.prepareSessionRegistration(requestSchemas.prepareSessionRegistration.parse(payload)); break;
        case 'createSession': result = embedded ? await embedded.createSession(requestSchemas.createSession.parse(payload), delivery ?? undefined) : await service.createSession(requestSchemas.createSession.parse(payload)); break;
        case 'addTab': result = embedded ? await embedded.addTab(requestSchemas.addTab.parse(payload), delivery ?? undefined) : await service.addTab(requestSchemas.addTab.parse(payload)); break;
        case 'renameTab': result = embedded ? await embedded.renameTab(requestSchemas.renameTab.parse(payload)) : failure('UNSUPPORTED', 'External tabs are read-only.'); break;
        case 'reorderTabs': result = embedded ? await embedded.reorderTabs(requestSchemas.reorderTabs.parse(payload)) : failure('UNSUPPORTED', 'External tabs are read-only.'); break;
        case 'focusSession': result = await service.focusSession(requestSchemas.focusSession.parse(payload)); break;
        case 'renameSession': result = await service.renameSession(requestSchemas.renameSession.parse(payload)); break;
        case 'setSessionPinned': result = await service.setSessionPinned(requestSchemas.setSessionPinned.parse(payload)); break;
        case 'settleSession': result = await service.settleSession(requestSchemas.settleSession.parse(payload)); break;
        case 'unsettleSession': result = await service.unsettleSession(requestSchemas.unsettleSession.parse(payload)); break;
        case 'deleteSession': result = await service.deleteSession(requestSchemas.deleteSession.parse(payload)); break;
        case 'clearSessionError': result = await service.clearSessionError(requestSchemas.clearSessionError.parse(payload)); break;
        case 'retryTab': result = await service.retryTab(requestSchemas.retryTab.parse(payload)); break;
        case 'saveSettings': result = await service.saveSettings(requestSchemas.saveSettings.parse(payload)); break;
        case 'setExplorerIntegration': result = await service.setExplorerIntegration(requestSchemas.setExplorerIntegration.parse(payload)); break;
        case 'setSessionEnv': result = await service.setSessionEnv(requestSchemas.setSessionEnv.parse(payload)); break;
        case 'setCliIntegration': result = await service.setCliIntegration(requestSchemas.setCliIntegration.parse(payload)); break;
        case 'getKeepAwake': result = keepAwake ? success({ active: keepAwake.active }) : failure('UNSUPPORTED', 'Keep awake is unavailable.'); break;
        case 'setKeepAwake': result = keepAwake ? success({ active: keepAwake.set(requestSchemas.setKeepAwake.parse(payload).active) }) : failure('UNSUPPORTED', 'Keep awake is unavailable.'); break;
        case 'getHomeDirectory': result = getHomeDirectory(); break;
        case 'resolveDirectory': result = await resolveDirectory(requestSchemas.resolveDirectory.parse(payload)); break;
        case 'createDirectory': result = await createDirectory(requestSchemas.createDirectory.parse(payload)); break;
        case 'completeDirectory': result = await completeDirectory(requestSchemas.completeDirectory.parse(payload)); break;
      }
      const response = responseSchemas[method].safeParse(result);
      return response.success ? response.data : failure('INTERNAL', 'Invalid application response');
    } catch { return failure('STORAGE_FAILED', 'The request could not be completed', true); }
  });
  const unsubscribe = service.subscribe(event => {
    sendToRenderer(window, rendererUrl, 'manager:changed', event);
  });
  const unsubscribeKeepAwake = keepAwake?.subscribe(state => {
    sendToRenderer(window, rendererUrl, 'manager:keep-awake', state);
  });
  const resetDelivery = () => { delivery?.dispose(); delivery = stopped ? null : embedded?.createTerminalView(sendTerminal) ?? null; };
  const resetViews = () => {
    resetDelivery();
    try { embedded?.setVisibleSessions({ sessionIds: [] }); } catch { /* Renderer teardown must still finish. */ }
  };
  window.webContents.on?.('render-process-gone', resetViews);
  window.webContents.on?.('destroyed', resetViews);
  // did-navigate excludes same-document/subframe and aborted navigation.
  window.webContents.on?.('did-navigate', resetViews);
  window.webContents.on?.('did-start-navigation', resetDelivery);
  return () => { stopped = true; unsubscribe(); unsubscribeKeepAwake?.(); resetViews(); window.webContents.removeListener?.('destroyed', resetViews); window.webContents.removeListener?.('render-process-gone', resetViews); window.webContents.removeListener?.('did-navigate', resetViews); window.webContents.removeListener?.('did-start-navigation', resetDelivery); ipcMain.removeHandler('manager:request'); };
}
