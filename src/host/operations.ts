import { failure, success, type KeepAwakeState } from '../shared/contracts';
import { requestSchemas, type ManagerMethod } from '../shared/schemas';
import type { EmbeddedSessionService } from '../main/terminal/service';
import type { TerminalControlView } from '../main/terminal/control';
import { getHomeDirectory, resolveDirectory, createDirectory, completeDirectory } from '../main/directory-prompt';
import type { SshProfileStore } from '../main/ssh/storage';
import { readPuttySessions } from '../main/ssh/putty';

export type HostSessionService = Pick<EmbeddedSessionService,
  'subscribe' | 'createTerminalView' | 'getSessionStats' | 'getSnapshot' | 'getTerminalProfiles' |
  'closeTab' | 'createSession' | 'addTab' | 'activateSession' | 'focusSession' |
  'refreshSessionMembership' | 'prepareSessionRegistration' | 'renameSession' | 'renameTab' | 'reorderTabs' |
  'setSessionPinned' | 'settleSession' | 'unsettleSession' | 'deleteSession' | 'clearSessionError' | 'retryTab' |
  'setSessionEnv' | 'saveSettings' | 'setExplorerIntegration' | 'setCliIntegration' |
  'setRemoteVisibleSessions' | 'clearRemoteVisibleSessions'>;
export interface HostKeepAwake {
  readonly active: boolean;
  readonly supported?: boolean;
  set(active: boolean): boolean | Promise<boolean>;
  subscribe(listener: (state: KeepAwakeState) => void): () => void;
}
export interface ManagerContext {
  service: HostSessionService; delivery: TerminalControlView; clientId: string;
  keepAwake?: HostKeepAwake; ssh?: SshProfileStore;
  desktop?: (method: ManagerMethod, payload: unknown) => Promise<unknown>;
}
export async function managerOperation(ctx: ManagerContext, method: ManagerMethod, payload: unknown): Promise<unknown> {
  const { service, delivery } = ctx;
  switch (method) {
    case 'getSnapshot': return service.getSnapshot();
    case 'getSessionStats': return service.getSessionStats(requestSchemas.getSessionStats.parse(payload));
    case 'getTerminalProfiles': return service.getTerminalProfiles();
    case 'setVisibleSessions': return service.setRemoteVisibleSessions(ctx.clientId, requestSchemas.setVisibleSessions.parse(payload));
    case 'attachTerminal': return delivery.attach(requestSchemas.attachTerminal.parse(payload));
    case 'takeTerminalControl': return delivery.take(requestSchemas.takeTerminalControl.parse(payload));
    case 'acknowledgeTerminal': return delivery.acknowledge(requestSchemas.acknowledgeTerminal.parse(payload));
    case 'detachTerminal': return delivery.detach(requestSchemas.detachTerminal.parse(payload));
    case 'writeTerminal': return delivery.write(requestSchemas.writeTerminal.parse(payload));
    case 'resizeTerminal': return delivery.resize(requestSchemas.resizeTerminal.parse(payload));
    case 'closeTab': return service.closeTab(requestSchemas.closeTab.parse(payload));
    case 'createSession': return service.createSession(requestSchemas.createSession.parse(payload));
    case 'addTab': return service.addTab(requestSchemas.addTab.parse(payload));
    case 'activateSession': return service.activateSession(requestSchemas.activateSession.parse(payload));
    case 'focusSession': return service.focusSession(requestSchemas.focusSession.parse(payload));
    case 'refreshSessionMembership': return service.refreshSessionMembership(requestSchemas.refreshSessionMembership.parse(payload));
    case 'prepareSessionRegistration': return service.prepareSessionRegistration(requestSchemas.prepareSessionRegistration.parse(payload));
    case 'renameSession': return service.renameSession(requestSchemas.renameSession.parse(payload));
    case 'renameTab': return service.renameTab(requestSchemas.renameTab.parse(payload));
    case 'reorderTabs': return service.reorderTabs(requestSchemas.reorderTabs.parse(payload));
    case 'setSessionPinned': return service.setSessionPinned(requestSchemas.setSessionPinned.parse(payload));
    case 'settleSession': return service.settleSession(requestSchemas.settleSession.parse(payload));
    case 'unsettleSession': return service.unsettleSession(requestSchemas.unsettleSession.parse(payload));
    case 'deleteSession': return service.deleteSession(requestSchemas.deleteSession.parse(payload));
    case 'clearSessionError': return service.clearSessionError(requestSchemas.clearSessionError.parse(payload));
    case 'retryTab': return service.retryTab(requestSchemas.retryTab.parse(payload));
    case 'setSessionEnv': return service.setSessionEnv(requestSchemas.setSessionEnv.parse(payload));
    case 'saveSettings': return service.saveSettings(requestSchemas.saveSettings.parse(payload));
    case 'setExplorerIntegration': return service.setExplorerIntegration(requestSchemas.setExplorerIntegration.parse(payload));
    case 'setCliIntegration': return service.setCliIntegration(requestSchemas.setCliIntegration.parse(payload));
    case 'getHomeDirectory': return getHomeDirectory();
    case 'resolveDirectory': return resolveDirectory(requestSchemas.resolveDirectory.parse(payload));
    case 'createDirectory': return createDirectory(requestSchemas.createDirectory.parse(payload));
    case 'completeDirectory': return completeDirectory(requestSchemas.completeDirectory.parse(payload));
    case 'getKeepAwake': return success({ active: ctx.keepAwake?.active ?? false });
    case 'setKeepAwake': {
      if (!ctx.keepAwake || ctx.keepAwake.supported === false) return failure('UNSUPPORTED', 'Keep awake is unavailable.');
      try { return success({ active: await ctx.keepAwake.set(requestSchemas.setKeepAwake.parse(payload).active) }); }
      catch (error) { return failure('NATIVE_UNAVAILABLE', error instanceof Error ? error.message : 'OS sleep inhibition failed.'); }
    }
    case 'listSshProfiles': return ctx.ssh ? ctx.ssh.list() : failure('UNSUPPORTED', 'SSH storage is unavailable.');
    case 'saveSshProfile': return ctx.ssh ? ctx.ssh.save(requestSchemas.saveSshProfile.parse(payload)) : failure('UNSUPPORTED', 'SSH storage is unavailable.');
    case 'deleteSshProfile': return ctx.ssh ? ctx.ssh.delete(requestSchemas.deleteSshProfile.parse(payload).id) : failure('UNSUPPORTED', 'SSH storage is unavailable.');
    case 'importPuttySessions': return ctx.ssh ? ctx.ssh.import(await readPuttySessions()) : failure('UNSUPPORTED', 'SSH storage is unavailable.');
    case 'chooseDirectory': case 'chooseSshKeyFile': case 'copyText': case 'readClipboardText':
    case 'openSessionFolder': case 'openExternal': case 'getUpdateStatus': case 'downloadUpdate': case 'installUpdate':
      return ctx.desktop ? ctx.desktop(method, payload) : failure('UNSUPPORTED', 'This action requires a desktop attached to the host.');
  }
}
