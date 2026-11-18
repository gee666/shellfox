import { RemoteClient } from './client';
import type { NativeRemoteTransport } from './protocol';
import type { HostClient, RemoteClientOptions } from './host-client';
import { nativeAvailability, TRANSITION_ENGINE, type EngineSelection } from '../rust-client/availability';
import { RustHostClient } from '../rust-client/rust-host-client';

// Explicit transition default until BOTH native platforms pass acceptance.
// Change only through an explicit application choice, never on module detection.
let selection: EngineSelection = TRANSITION_ENGINE;
export function setMobileClientEngine(next: EngineSelection): void { selection = next; }
export function mobileClientEngine(): EngineSelection { return selection; }

export function createHostClient(peerId: string, transport: NativeRemoteTransport,
  options: RemoteClientOptions, engine: EngineSelection = selection,
): HostClient {
  if (engine.engine === 'webview-transition') return new RemoteClient(peerId, transport, options);
  const availability = nativeAvailability(engine);
  if (!availability.available) throw new Error(availability.reason);
  return new RustHostClient(peerId, availability.native, options);
}

export type { HostClient, ClientState, RemoteClientOptions } from './host-client';
export type { EngineSelection } from '../rust-client/availability';
