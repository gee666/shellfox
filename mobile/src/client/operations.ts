import type { RemoteMethod, RemoteRequestPayload, RemoteResponse } from '../../../src/shared/remote-contracts';

/** Convenience methods retain the shared request and Result types. */
export abstract class RemoteOperations {
  abstract request<M extends RemoteMethod>(method: M, payload: RemoteRequestPayload<M>): Promise<RemoteResponse<M>>;

  getSnapshot() { return this.request('getSnapshot', {}); }
  getTerminalProfiles() { return this.request('getTerminalProfiles', {}); }
  getHomeDirectory() { return this.request('getHomeDirectory', {}); }
  getSessionStats(input: RemoteRequestPayload<'getSessionStats'> = {}) { return this.request('getSessionStats', input); }
  createSession(input: RemoteRequestPayload<'createSession'>) { return this.request('createSession', input); }
  addTab(input: RemoteRequestPayload<'addTab'>) { return this.request('addTab', input); }
  activateSession(input: RemoteRequestPayload<'activateSession'>) { return this.request('activateSession', input); }
  renameSession(input: RemoteRequestPayload<'renameSession'>) { return this.request('renameSession', input); }
  renameTab(input: RemoteRequestPayload<'renameTab'>) { return this.request('renameTab', input); }
  reorderTabs(input: RemoteRequestPayload<'reorderTabs'>) { return this.request('reorderTabs', input); }
  setSessionPinned(input: RemoteRequestPayload<'setSessionPinned'>) { return this.request('setSessionPinned', input); }
  settleSession(input: RemoteRequestPayload<'settleSession'>) { return this.request('settleSession', input); }
  unsettleSession(input: RemoteRequestPayload<'unsettleSession'>) { return this.request('unsettleSession', input); }
  deleteSession(input: RemoteRequestPayload<'deleteSession'>) { return this.request('deleteSession', input); }
  clearSessionError(input: RemoteRequestPayload<'clearSessionError'>) { return this.request('clearSessionError', input); }
  setSessionEnv(input: RemoteRequestPayload<'setSessionEnv'>) { return this.request('setSessionEnv', input); }
  setVisibleSessions(input: RemoteRequestPayload<'setVisibleSessions'>) { return this.request('setVisibleSessions', input); }
  resolveDirectory(input: RemoteRequestPayload<'resolveDirectory'>) { return this.request('resolveDirectory', input); }
  createDirectory(input: RemoteRequestPayload<'createDirectory'>) { return this.request('createDirectory', input); }
  completeDirectory(input: RemoteRequestPayload<'completeDirectory'>) { return this.request('completeDirectory', input); }
  attachTerminal(input: RemoteRequestPayload<'attachTerminal'>) { return this.request('attachTerminal', input); }
  writeTerminal(input: RemoteRequestPayload<'writeTerminal'>) { return this.request('writeTerminal', input); }
  resizeTerminal(input: RemoteRequestPayload<'resizeTerminal'>) { return this.request('resizeTerminal', input); }
  acknowledgeTerminal(input: RemoteRequestPayload<'acknowledgeTerminal'>) { return this.request('acknowledgeTerminal', input); }
  detachTerminal(input: RemoteRequestPayload<'detachTerminal'>) { return this.request('detachTerminal', input); }
  closeTab(input: RemoteRequestPayload<'closeTab'>) { return this.request('closeTab', input); }
}
