import type { RemoteClientOptions } from '../client/host-client';
import { nativeAvailability, probeNativeClient, type EngineSelection } from './availability';
import { RustHostClient } from './rust-host-client';

export function createNativeV1Client(peerId: string, options: RemoteClientOptions, selection?: EngineSelection) {
  const availability = selection === undefined ? probeNativeClient() : nativeAvailability(selection);
  if (!availability.available) throw new Error(availability.reason);
  return new RustHostClient(peerId, availability.native, options);
}
