import type { ChangedEvent, KeepAwakeState, ManagerApi, TerminalApi } from '../shared/contracts';
import type { RemoteApi, RemoteEvent, RemoteMethod, RemoteResponse, RemoteRequestPayload } from '../shared/remote-contracts';
import { responseSchemas } from '../shared/schemas';
import type { HostEndpoint, HostEvent, HostMethod, HostRequest, HostResponse } from '../shared/host-contracts';
import { hostEndpointSchema } from '../shared/host-schemas';
import { readPrivateEndpoint } from '../main/control-endpoint';
import { hostEndpointPath } from './rpc';
import { hostRequest, HostTransportError } from './client-transport';

type TerminalListener = Parameters<TerminalApi['subscribeTerminal']>[0];
export class HostClient {
  readonly service: ManagerApi & TerminalApi;
  readonly remote: RemoteApi;
  private readonly changed = new Set<(event: ChangedEvent) => void>();
  private readonly terminals = new Set<TerminalListener>();
  private readonly remotes = new Set<(event: RemoteEvent) => void>();
  private readonly awake = new Set<(event: KeepAwakeState) => void>();
  private readonly abort = new AbortController();
  private polling?: Promise<void>;
  private closed = false;
  private disconnectError?: Error;
  private readonly disconnected = new Set<(error: Error) => void>();
  onDisconnect(listener: (error: Error) => void): () => void {
    this.disconnected.add(listener); this.startPolling();
    if (this.disconnectError) listener(this.disconnectError);
    return () => { this.disconnected.delete(listener); };
  }
  private lost(error: unknown): void {
    if (this.closed || this.disconnectError) return;
    this.disconnectError = error instanceof Error ? error : new Error('Host connection lost.');
    for (const listener of this.disconnected) { try { listener(this.disconnectError); } catch { /* Observers cannot prevent terminal cleanup. */ } }
  }
  private constructor(private readonly endpoint: HostEndpoint, private readonly clientId: string) {
    const subscribe = <T>(listeners: Set<(event: T) => void>, listener: (event: T) => void) => { listeners.add(listener); this.startPolling(); return () => { listeners.delete(listener); }; };
    this.service = {
      getSnapshot: () => this.request('getSnapshot'),
      getSessionStats: input => this.request('getSessionStats', input),
      setVisibleSessions: input => this.request('setVisibleSessions', input),
      createSession: input => this.request('createSession', input),
      addTab: input => this.request('addTab', input),
      activateSession: input => this.request('activateSession', input),
      focusSession: input => this.request('focusSession', input),
      refreshSessionMembership: input => this.request('refreshSessionMembership', input),
      prepareSessionRegistration: input => this.request('prepareSessionRegistration', input),
      renameSession: input => this.request('renameSession', input),
      renameTab: input => this.request('renameTab', input),
      reorderTabs: input => this.request('reorderTabs', input),
      setSessionPinned: input => this.request('setSessionPinned', input),
      settleSession: input => this.request('settleSession', input),
      unsettleSession: input => this.request('unsettleSession', input),
      deleteSession: input => this.request('deleteSession', input),
      clearSessionError: input => this.request('clearSessionError', input),
      retryTab: input => this.request('retryTab', input),
      setSessionEnv: input => this.request('setSessionEnv', input),
      saveSettings: input => this.request('saveSettings', input),
      setExplorerIntegration: input => this.request('setExplorerIntegration', input),
      setCliIntegration: input => this.request('setCliIntegration', input),
      getHomeDirectory: () => this.request('getHomeDirectory'),
      resolveDirectory: input => this.request('resolveDirectory', input),
      createDirectory: input => this.request('createDirectory', input),
      completeDirectory: input => this.request('completeDirectory', input),
      listSshProfiles: () => this.request('listSshProfiles'),
      saveSshProfile: input => this.request('saveSshProfile', input),
      deleteSshProfile: input => this.request('deleteSshProfile', input),
      importPuttySessions: () => this.request('importPuttySessions'),
      chooseSshKeyFile: () => this.request('chooseSshKeyFile'),
      chooseDirectory: () => this.request('chooseDirectory'),
      copyText: input => this.request('copyText', input),
      readClipboardText: () => this.request('readClipboardText'),
      openSessionFolder: input => this.request('openSessionFolder', input),
      openExternal: input => this.request('openExternal', input),
      getKeepAwake: () => this.request('getKeepAwake'),
      setKeepAwake: input => this.request('setKeepAwake', input),
      getTerminalProfiles: () => this.request('getTerminalProfiles'),
      attachTerminal: input => this.request('attachTerminal', input),
      writeTerminal: input => this.request('writeTerminal', input),
      resizeTerminal: input => this.request('resizeTerminal', input),
      closeTab: input => this.request('closeTab', input),
      acknowledgeTerminal: input => this.request('acknowledgeTerminal', input),
      detachTerminal: input => this.request('detachTerminal', input),
      getUpdateStatus: () => this.request('getUpdateStatus'),
      downloadUpdate: () => this.request('downloadUpdate'),
      installUpdate: input => this.request('installUpdate', input),
      subscribe: listener => subscribe(this.changed, listener),
      subscribeTerminal: listener => subscribe(this.terminals, listener),
      subscribeKeepAwake: listener => subscribe(this.awake, listener),
    };
    this.remote = {
      getState: () => this.request('remote.getState'),
      configure: input => this.request('remote.configure', input),
      login: input => this.request('remote.login', input ?? {}),
      connect: input => this.request('remote.connect', input),
      disconnect: input => this.request('remote.disconnect', input),
      approve: input => this.request('remote.approve', input),
      reject: input => this.request('remote.reject', input),
      revoke: input => this.request('remote.revoke', input),
      forget: input => this.request('remote.forget', input),
      request: input => this.forward(input),
      subscribe: listener => subscribe(this.remotes, listener),
    };
  }
  static async connect(userData: string): Promise<HostClient> {
    const endpoint = await readPrivateEndpoint(hostEndpointPath(userData), hostEndpointSchema);
    const opened = await hostRequest(endpoint, undefined, 'host.open', {});
    const client = new HostClient(endpoint, opened.clientId);
    try { await client.status(); return client; }
    catch (error) { await client.close(); throw error; }
  }
  request<M extends HostMethod>(method: M, payload?: HostRequest<M>): Promise<HostResponse<M>> {
    if (this.closed) return Promise.reject(new Error('Host client is closed.'));
    if (this.disconnectError) return Promise.reject(this.disconnectError);
    // hostRequest validates the default empty object against the selected method.
    return hostRequest(this.endpoint, this.clientId, method, payload ?? {}, this.abort.signal).catch((error: unknown) => { if (error instanceof HostTransportError) this.lost(error); throw error; });
  }
  private forward<M extends RemoteMethod>(input: { peerId: string; method: M; payload: RemoteRequestPayload<M> }): Promise<RemoteResponse<M>>;
  private async forward(input: { peerId: string; method: RemoteMethod; payload: RemoteRequestPayload<RemoteMethod> }): Promise<RemoteResponse<RemoteMethod>> {
    return responseSchemas[input.method].parse(await this.request('remote.request', input));
  }
  status() { return this.request('host.status'); }
  async stop(): Promise<void> {
    const result = await this.request('host.stop', { wait: true });
    if (!('stopped' in result)) throw new Error('Host stop was accepted but did not confirm completion.');
  }
  async poll(): Promise<{ events: HostEvent[] }> { return this.request('host.poll'); }
  private startPolling(): void {
    if (this.polling || this.closed) return;
    this.polling = (async () => {
      try {
        while (!this.closed && (this.changed.size || this.terminals.size || this.remotes.size || this.awake.size || this.disconnected.size)) {
          const { events } = await this.poll(); for (const event of events) this.emit(event);
        }
      } catch (error) { this.lost(error); }
      finally { this.polling = undefined; }
    })();
  }
  private emit(event: HostEvent): void {
    const send = <T>(listeners: Set<(value: T) => void>, value: T) => { for (const listener of listeners) { try { listener(value); } catch { /* One observer cannot block other views. */ } } };
    switch (event.type) {
      case 'changed': send(this.changed, event.event); break;
      case 'terminal': send(this.terminals, event.event); break;
      case 'remote': send(this.remotes, event.event); break;
      case 'keepAwake': send(this.awake, event.event); break;
    }
  }
  async close(): Promise<void> {
    if (this.closed) return; this.closed = true; this.abort.abort();
    await hostRequest(this.endpoint, this.clientId, 'host.close', {}).catch(() => {});
    this.changed.clear(); this.terminals.clear(); this.remotes.clear(); this.awake.clear(); this.disconnected.clear(); await this.polling;
  }
}
export const connectHostClient = (userData: string): Promise<HostClient> => HostClient.connect(userData);
