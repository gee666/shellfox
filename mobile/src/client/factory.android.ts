import { createNativeV1Client } from '../rust-client/create-v1-client';
import type { EngineSelection } from '../rust-client/availability';
import type { HostClient, RemoteClientOptions } from './host-client';
import type { NativeRemoteTransport } from './protocol';

export function createHostClient(peerId: string, _transport: NativeRemoteTransport,
  options: RemoteClientOptions, engine?: EngineSelection,
): HostClient {
  return createNativeV1Client(peerId, options, engine);
}

export type { HostClient, ClientState, RemoteClientOptions } from './host-client';
export type { EngineSelection } from '../rust-client/availability';
