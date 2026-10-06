import { contextBridge, ipcRenderer } from 'electron';
import type { ChangedEvent, ManagerApi, TerminalEvent } from '../shared/contracts';
import { failure } from '../shared/contracts';
import { changedEventSchema, terminalEventSchema, requestSchemas, responseSchemas, type ManagerMethod } from '../shared/schemas';
async function request(method: ManagerMethod, payload: unknown): Promise<any> {
  if (!requestSchemas[method].safeParse(payload).success) return failure('VALIDATION', 'Invalid request');
  try {
    const response = responseSchemas[method].safeParse(await ipcRenderer.invoke('manager:request', { version: 1, method, payload }));
    return response.success ? response.data : failure('INTERNAL', 'Invalid application response');
  } catch { return failure('INTERNAL', 'Application request failed', true); }
}
const api: ManagerApi = {
  getSnapshot: () => request('getSnapshot', {}),
  getTerminalProfiles: () => request('getTerminalProfiles', {}),
  attachTerminal: input => request('attachTerminal', input),
  writeTerminal: input => request('writeTerminal', input),
  resizeTerminal: input => request('resizeTerminal', input),
  closeTab: input => request('closeTab', input),
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
  getHistory: input => request('getHistory', input),
  saveSettings: input => request('saveSettings', input),
  setExplorerIntegration: input => request('setExplorerIntegration', input),
  copyText: input => request('copyText', input),
  openSessionFolder: input => request('openSessionFolder', input),
  setSessionEnv: input => request('setSessionEnv', input),
  setCliIntegration: input => request('setCliIntegration', input),
  chooseDirectory: () => request('chooseDirectory', {}),
  getUpdateStatus: () => request('getUpdateStatus', {}),
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
