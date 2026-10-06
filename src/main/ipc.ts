import { ipcMain, dialog, clipboard, shell, BrowserWindow, type IpcMainInvokeEvent } from 'electron';
import { failure, success } from '../shared/contracts';
import { requestEnvelopeSchema, requestSchemas, responseSchemas } from '../shared/schemas';
import type { SessionService } from './service';
import type { EmbeddedSessionService } from './terminal/service';
import { TerminalDelivery } from './terminal/delivery';
import { validateDirectory } from './directory';
import type { UpdateChecker } from './update-check';

export function isTrustedSender(event: IpcMainInvokeEvent, window: BrowserWindow, rendererUrl: string): boolean {
  return !window.isDestroyed() && event.sender === window.webContents && !!event.senderFrame &&
    event.senderFrame === window.webContents.mainFrame && event.senderFrame.url === rendererUrl;
}
export function installIpc(window: BrowserWindow, rendererUrl: string, service: SessionService | EmbeddedSessionService, rendererReady?: () => void, updates?: UpdateChecker): () => void {
  let announcedReady = false;
  const embedded = 'attachTerminal' in service ? service : null;
  const delivery = embedded ? new TerminalDelivery(input => embedded.attachTerminal(input), event => {
    if (!window.isDestroyed() && window.webContents.mainFrame.url === rendererUrl) window.webContents.send('manager:terminal', event);
  }) : null;
  const unsubscribeTerminal = embedded?.subscribeTerminal(event => {
    if (event.type !== 'activity') { delivery?.event(event); return; }
    // Activity is global metadata, not an attached/credited output stream.
    for (const view of BrowserWindow.getAllWindows()) {
      if (!view.isDestroyed() && view.webContents.mainFrame.url === rendererUrl) view.webContents.send('manager:terminal', event);
    }
  });
  ipcMain.handle('manager:request', async (event, raw: unknown) => {
    if (!isTrustedSender(event, window, rendererUrl)) return failure('AUTH_FAILED', 'Request did not come from the application renderer');
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
      let result: unknown;
      // Keep methods explicit. There is no property lookup into the service.
      switch (method) {
        case 'getSnapshot':
          if (embedded) await embedded.refreshIntegrations();
          result = service.getSnapshot();
          // The renderer subscribes before its first snapshot request. Load completion alone is not enough.
          if (!announcedReady) { announcedReady = true; rendererReady?.(); }
          break;
        case 'getTerminalProfiles': result = embedded ? embedded.getTerminalProfiles() : failure('UNSUPPORTED', 'Embedded terminals are unavailable in this backend.'); break;
        case 'attachTerminal': {
          const attachment = embedded ? embedded.attachTerminal(requestSchemas.attachTerminal.parse(payload)) : failure('UNSUPPORTED', 'Embedded terminals are unavailable in this backend.');
          const accepted = attachment.ok ? delivery?.attached(attachment.value) : null;
          result = accepted && !accepted.ok ? accepted : attachment; break;
        }
        case 'writeTerminal': result = embedded ? embedded.writeTerminal(requestSchemas.writeTerminal.parse(payload)) : failure('UNSUPPORTED', 'Embedded terminals are unavailable in this backend.'); break;
        case 'resizeTerminal': result = embedded ? embedded.resizeTerminal(requestSchemas.resizeTerminal.parse(payload)) : failure('UNSUPPORTED', 'Embedded terminals are unavailable in this backend.'); break;
        case 'closeTab': result = embedded ? await embedded.closeTab(requestSchemas.closeTab.parse(payload)) : failure('UNSUPPORTED', 'External processes cannot be closed by embedded terminal controls.'); break;
        case 'acknowledgeTerminal': result = delivery ? delivery.acknowledge(requestSchemas.acknowledgeTerminal.parse(payload)) : failure('UNSUPPORTED', 'Embedded terminals are unavailable in this backend.'); break;
        case 'detachTerminal': result = delivery ? delivery.detach(requestSchemas.detachTerminal.parse(payload)) : failure('UNSUPPORTED', 'Embedded terminals are unavailable in this backend.'); break;
        case 'activateSession': result = await service.activateSession(requestSchemas.activateSession.parse(payload)); break;
        case 'refreshSessionMembership': result = await service.refreshSessionMembership(requestSchemas.refreshSessionMembership.parse(payload)); break;
        case 'prepareSessionRegistration': result = await service.prepareSessionRegistration(requestSchemas.prepareSessionRegistration.parse(payload)); break;
        case 'createSession': result = await service.createSession(requestSchemas.createSession.parse(payload)); break;
        case 'addTab': result = await service.addTab(requestSchemas.addTab.parse(payload)); break;
        case 'focusSession': result = await service.focusSession(requestSchemas.focusSession.parse(payload)); break;
        case 'renameSession': result = await service.renameSession(requestSchemas.renameSession.parse(payload)); break;
        case 'setSessionPinned': result = await service.setSessionPinned(requestSchemas.setSessionPinned.parse(payload)); break;
        case 'settleSession': result = await service.settleSession(requestSchemas.settleSession.parse(payload)); break;
        case 'unsettleSession': result = await service.unsettleSession(requestSchemas.unsettleSession.parse(payload)); break;
        case 'clearSessionError': result = await service.clearSessionError(requestSchemas.clearSessionError.parse(payload)); break;
        case 'retryTab': result = await service.retryTab(requestSchemas.retryTab.parse(payload)); break;
        case 'getHistory': result = service.getHistory(requestSchemas.getHistory.parse(payload)); break;
        case 'saveSettings': result = await service.saveSettings(requestSchemas.saveSettings.parse(payload)); break;
        case 'setExplorerIntegration': result = await service.setExplorerIntegration(requestSchemas.setExplorerIntegration.parse(payload)); break;
        case 'setSessionEnv': result = await service.setSessionEnv(requestSchemas.setSessionEnv.parse(payload)); break;
        case 'setCliIntegration': result = await service.setCliIntegration(requestSchemas.setCliIntegration.parse(payload)); break;
        case 'copyText': {
          try { await clipboard.writeText(requestSchemas.copyText.parse(payload).text); result = success({ copied: true }); }
          catch { result = failure('INTERNAL', 'Could not copy text to the system clipboard.', true); }
          break;
        }
        case 'openSessionFolder': {
          const { sessionId } = requestSchemas.openSessionFolder.parse(payload);
          const saved = service.repository.session(sessionId);
          if (!saved) { result = failure('NOT_FOUND', 'Session not found.'); break; }
          try {
            const cwd = await validateDirectory(saved.cwd), error = await shell.openPath(cwd);
            result = error ? failure('INTERNAL', 'The session folder could not be opened: ' + error) : success({ opened: true });
          } catch { result = failure('NOT_FOUND', 'The session folder is missing or inaccessible.'); }
          break;
        }
        case 'getUpdateStatus': result = updates ? success(await updates.status()) : failure('UNSUPPORTED', 'Update checks are unavailable.'); break;
        case 'chooseDirectory': {
          const selection = await dialog.showOpenDialog(window, { properties: ['openDirectory'], title: 'Choose default working directory' });
          if (selection.canceled || !selection.filePaths[0]) result = success(null);
          else {
            try { result = success({ cwd: await validateDirectory(selection.filePaths[0]) }); }
            catch { result = failure('VALIDATION', 'Choose an accessible local directory'); }
          }
          break;
        }
      }
      const response = responseSchemas[method].safeParse(result);
      return response.success ? response.data : failure('INTERNAL', 'Invalid application response');
    } catch { return failure('STORAGE_FAILED', 'The request could not be completed', true); }
  });
  const unsubscribe = service.subscribe(event => {
    if (!window.isDestroyed()) window.webContents.send('manager:changed', event);
  });
  const resetViews = () => delivery?.dispose();
  window.webContents.on?.('render-process-gone', resetViews);
  window.webContents.on?.('did-start-navigation', resetViews);
  return () => { unsubscribe(); unsubscribeTerminal?.(); delivery?.dispose(); window.webContents.removeListener?.('render-process-gone', resetViews); window.webContents.removeListener?.('did-start-navigation', resetViews); ipcMain.removeHandler('manager:request'); };
}
