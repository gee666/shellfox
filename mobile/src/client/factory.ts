import { RemoteClient } from './client';
import type { NativeRemoteTransport } from './protocol';
import type { HostClient, RemoteClientOptions } from './host-client';
import type { EngineSelection } from '../rust-client/availability';
import { createNativeV1Client } from '../rust-client/create-v1-client';

export function createHostClient(peerId: string, transport: NativeRemoteTransport,
  options: RemoteClientOptions, engine?: EngineSelection,
): HostClient {
  return engine?.engine === 'rust-native' ? createNativeV1Client(peerId, options, engine)
    : new RemoteClient(peerId, transport, options);
}

export type { HostClient, ClientState, RemoteClientOptions } from './host-client';
export type { EngineSelection } from '../rust-client/availability';
