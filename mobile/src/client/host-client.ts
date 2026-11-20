import type { AppError, TerminalApi, TerminalEvent } from '../../../src/shared/contracts';
import type { RemoteHostEvent, RemotePeer } from '../../../src/shared/remote-contracts';
import type { RemoteOperations } from './operations';

export interface ClientState {
  readonly peerId: string;
  readonly status: RemotePeer['connection'];
  readonly lastError: AppError | null;
  /** Changes on every successful pairing. Terminal views must reattach. */
  readonly connectionId: string | null;
}

export interface RemoteClientOptions {
  /** Supply cryptographically random UUIDs from the native platform. Never persist these. */
  createClientId(): string;
  approvalRetryMs?: number;
  reconnectDelayMs?: number;
  maxReconnectDelayMs?: number;
}

export type MobileTerminalApi = Pick<TerminalApi, 'attachTerminal' | 'writeTerminal' | 'resizeTerminal'
  | 'acknowledgeTerminal' | 'detachTerminal' | 'takeTerminalControl'>;

export interface HostClient extends Pick<RemoteOperations, keyof RemoteOperations> {
  readonly peerId: string;
  readonly terminalApi: MobileTerminalApi;
  subscribeTerminal(listener: (event: TerminalEvent) => void): () => void;
  getState(): ClientState;
  subscribe(listener: (event: RemoteHostEvent) => void): () => void;
  subscribeState(listener: (state: ClientState) => void): () => void;
  connect(): void;
  reconnect(): void;
  suspend(): void;
  stop(): void;
}
