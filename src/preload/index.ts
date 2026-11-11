import { createFileApi } from '../shared/file-api';
import { contextBridge, ipcRenderer } from 'electron';
import type { z } from 'zod';
import { closeConfirmationApi } from './close-confirmation';
import { remoteApi } from './remote';
import { desktopFilesApi } from './desktop-files';
import type { ChangedEvent, KeepAwakeState, ManagerApi, TerminalEvent } from '../shared/contracts';
import { failure } from '../shared/contracts';
import { changedEventSchema, keepAwakeStateSchema, terminalEventSchema, requestSchemas, responseSchemas, type ManagerMethod } from '../shared/schemas';
type ManagerResponse<M extends ManagerMethod = ManagerMethod> = z.output<(typeof responseSchemas)[M]>;
function request<M extends ManagerMethod>(method: M, payload: unknown): Promise<ManagerResponse<M>>;
async function request(method: ManagerMethod, payload: unknown): Promise<ManagerResponse> {
  if (!requestSchemas[method].safeParse(payload).success) return failure('VALIDATION', 'Invalid request');
  try {
    const response = responseSchemas[method].safeParse(await ipcRenderer.invoke('manager:request', { version: 1, method, payload }));
    return response.success ? response.data : failure('INTERNAL', 'Invalid application response');
  } catch { return failure('INTERNAL', 'Application request failed', true); }
}
const api: ManagerApi = {
  ...closeConfirmationApi,
  desktopFiles: desktopFilesApi,
  files: createFileApi({
    list: input => request('listFiles', input),
    beginRead: input => request('beginFileRead', input),
    readChunk: input => request('readFileChunk', input),
    beginWrite: input => request('beginFileWrite', input),
    writeChunk: input => request('writeFileChunk', input),
    finishWrite: input => request('finishFileWrite', input),
    cancelTransfer: input => request('cancelFileTransfer', input),
  }),
  listSshProfiles: () => request('listSshProfiles', {}),
  saveSshProfile: input => request('saveSshProfile', input),
  deleteSshProfile: input => request('deleteSshProfile', input),
  importPuttySessions: () => request('importPuttySessions', {}),
  chooseSshKeyFile: () => request('chooseSshKeyFile', {}),
  getSnapshot: () => request('getSnapshot', {}),
  getSessionStats: input => request('getSessionStats', input),
  setVisibleSessions: input => request('setVisibleSessions', input),
  getTerminalProfiles: () => request('getTerminalProfiles', {}),
  attachTerminal: input => request('attachTerminal', input),
  takeTerminalControl: input => request('takeTerminalControl', input),
  writeTerminal: input => request('writeTerminal', input),
  resizeTerminal: input => request('resizeTerminal', input),
  closeTab: input => request('closeTab', input),
  renameTab: input => request('renameTab', input),
  reorderTabs: input => request('reorderTabs', input),
  acknowledgeTerminal: input => request('acknowledgeTerminal', input),
  detachTerminal: input => request('detachTerminal', input),
  subscribeTerminal(listener: (event: TerminalEvent) => void): () => void {
    if (typeof listener !== 'function') return () => {};
    const receive = (_event: unknown, value: unknown) => {
      const parsed = terminalEventSchema.safeParse(value);
      if (parsed.success) listener(parsed.data);
    };
    ipcRenderer.on('manager:terminal', receive);
    return () => ipcRenderer.removeListener('manager:terminal', receive);
  },
  activateSession: input => request('activateSession', input),
  refreshSessionMembership: input => request('refreshSessionMembership', input),
  prepareSessionRegistration: input => request('prepareSessionRegistration', input),
  createSession: input => request('createSession', input),
  addTab: input => request('addTab', input),
  focusSession: input => request('focusSession', input),
  renameSession: input => request('renameSession', input),
  setSessionPinned: input => request('setSessionPinned', input),
  settleSession: input => request('settleSession', input),
  unsettleSession: input => request('unsettleSession', input),
  deleteSession: input => request('deleteSession', input),
  clearSessionError: input => request('clearSessionError', input),
  retryTab: input => request('retryTab', input),
  saveSettings: input => request('saveSettings', input),
  setExplorerIntegration: input => request('setExplorerIntegration', input),
  copyText: input => request('copyText', input),
  readClipboardText: () => request('readClipboardText', {}),
  openSessionFolder: input => request('openSessionFolder', input),
  openFileExplorer: input => request('openFileExplorer', input),
  openExternal: input => request('openExternal', input),
  getKeepAwake: () => request('getKeepAwake', {}),
  setKeepAwake: input => request('setKeepAwake', input),
  subscribeKeepAwake(listener: (state: KeepAwakeState) => void): () => void {
    if (typeof listener !== 'function') return () => {};
    const receive = (_event: unknown, value: unknown) => {
      const parsed = keepAwakeStateSchema.safeParse(value);
      if (parsed.success) listener(parsed.data);
    };
    ipcRenderer.on('manager:keep-awake', receive);
    return () => ipcRenderer.removeListener('manager:keep-awake', receive);
  },
  setSessionEnv: input => request('setSessionEnv', input),
  setTerminalEnv: input => request('setTerminalEnv', input),
  setCliIntegration: input => request('setCliIntegration', input),
  chooseDirectory: () => request('chooseDirectory', {}),
  getHomeDirectory: () => request('getHomeDirectory', {}),
  resolveDirectory: input => request('resolveDirectory', input),
  createDirectory: input => request('createDirectory', input),
  completeDirectory: input => request('completeDirectory', input),
  getUpdateStatus: () => request('getUpdateStatus', {}),
  downloadUpdate: () => request('downloadUpdate', {}),
  installUpdate: input => request('installUpdate', input),
  subscribe(listener: (event: ChangedEvent) => void): () => void {
    if (typeof listener !== 'function') return () => {};
    const receive = (_event: unknown, value: unknown) => {
      const parsed = changedEventSchema.safeParse(value);
      if (parsed.success) listener(parsed.data);
    };
    ipcRenderer.on('manager:changed', receive);
    return () => ipcRenderer.removeListener('manager:changed', receive);
  },
};
contextBridge.exposeInMainWorld('shellfox', Object.freeze(api));
contextBridge.exposeInMainWorld('shellfoxRemote', Object.freeze(remoteApi));
