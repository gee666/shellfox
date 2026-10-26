import { failure, type ManagerApi } from '../shared/contracts';
import type { RemoteApi, RemoteMethod, RemoteRequestPayload } from '../shared/remote-contracts';

/** Only session, terminal and directory operations may cross the network. */
export function createRemoteManagerApi(remote: RemoteApi, peerId: string, local: ManagerApi): ManagerApi {
  const unsupported = async () => failure('UNSUPPORTED', 'Not available for remote machines.');
  function forward<M extends RemoteMethod>(method: M, payload: RemoteRequestPayload<M>) {
    return remote.request({ peerId, method, payload });
  }
  return {
    getSnapshot: () => forward('getSnapshot', {}),
    getTerminalProfiles: () => forward('getTerminalProfiles', {}),
    attachTerminal: input => forward('attachTerminal', input),
    writeTerminal: input => forward('writeTerminal', input),
    resizeTerminal: input => forward('resizeTerminal', input),
    closeTab: input => forward('closeTab', input),
    acknowledgeTerminal: input => forward('acknowledgeTerminal', input),
    detachTerminal: input => forward('detachTerminal', input),
    createSession: input => forward('createSession', input),
    addTab: input => forward('addTab', input),
    activateSession: input => forward('activateSession', input),
    renameSession: input => forward('renameSession', input),
    renameTab: input => forward('renameTab', input),
    reorderTabs: input => forward('reorderTabs', input),
    setSessionPinned: input => forward('setSessionPinned', input),
    settleSession: input => forward('settleSession', input),
    unsettleSession: input => forward('unsettleSession', input),
    deleteSession: input => forward('deleteSession', input),
    clearSessionError: input => forward('clearSessionError', input),
    setSessionEnv: input => forward('setSessionEnv', input),
    getHomeDirectory: () => forward('getHomeDirectory', {}),
    resolveDirectory: input => forward('resolveDirectory', input),
    createDirectory: input => forward('createDirectory', input),
    getSessionStats: input => forward('getSessionStats', input),
    completeDirectory: input => forward('completeDirectory', input),
    subscribe: listener => remote.subscribe(event => {
      if (event.type === 'changed' && event.peerId === peerId) listener(event.event);
    }),
    subscribeTerminal: listener => remote.subscribe(event => {
      if (event.type === 'terminal' && event.peerId === peerId) listener(event.event);
    }),
    copyText: input => local.copyText(input),
    ...(local.readClipboardText ? { readClipboardText: () => local.readClipboardText!() } : {}),
    ...(local.openExternal ? { openExternal: (input: { url: string }) => local.openExternal!(input) } : {}),
    setVisibleSessions: input => forward('setVisibleSessions', input),
    refreshSessionMembership: unsupported,
    prepareSessionRegistration: unsupported,
    focusSession: unsupported,
    retryTab: unsupported,
    saveSettings: unsupported,
    setExplorerIntegration: unsupported,
    openSessionFolder: unsupported,
    setCliIntegration: unsupported,
    chooseDirectory: unsupported,
    listSshProfiles: unsupported,
    saveSshProfile: unsupported,
    deleteSshProfile: unsupported,
    importPuttySessions: unsupported,
    chooseSshKeyFile: unsupported,
  };
}
