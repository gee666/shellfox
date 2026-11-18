import { createHostClient, type HostClient, type EngineSelection, type RemoteClientOptions } from '../../client/factory';
import type { NativeRemoteTransport } from '../../client/protocol';
import { nativeAvailability } from '../availability';
import { ObserverV2Client } from './client';
import type { ObserverNativeBinding } from './native';
import { createManagerProfileClient } from '../manager-v2/factory';
import type { ManagerV2Client } from '../manager-v2/client';

export type ProfileSelection =
  | Readonly<{ profile: 'v1'; engine?: EngineSelection }>
  | Readonly<{ profile: 'observer-v2'; engine: Extract<EngineSelection, { engine: 'rust-native' }> }>
  | Readonly<{ profile: 'manager-v2'; engine: Extract<EngineSelection, { engine: 'rust-native' }> }>;
export type ProfileClient =
  | Readonly<{ profile: 'v1'; client: HostClient }>
  | Readonly<{ profile: 'observer-v2'; client: ObserverV2Client }>
  | Readonly<{ profile: 'manager-v2'; client: ManagerV2Client }>;

/** Separate factory result. Observer facts are never coerced into a v1 MachineView. */
export async function createProfileClient(peerId: string, transport: NativeRemoteTransport,
  options: RemoteClientOptions, selection: ProfileSelection = { profile: 'v1' },
): Promise<ProfileClient> {
  if (selection.profile === 'v1') return { profile: 'v1', client: createHostClient(peerId, transport, options, selection.engine) };
  if(selection.profile==='manager-v2')return {profile:'manager-v2',client:await createManagerProfileClient(peerId,options.createClientId(),selection.engine)};
  const available = nativeAvailability(selection.engine);
  if (!available.available) throw new Error(available.reason);
  const native = available.native;
  const methods: ReadonlyArray<keyof ObserverNativeBinding> = ['observerV2AbiVersion', 'clientProfile',
    'createObserverV2Client', 'discoverObserverV2', 'pairObserverV2', 'observerV2Operation',
    'pollObserverV2', 'reconnectObserverV2', 'disconnectObserverV2'];
  if (methods.some(method => typeof native[method] !== 'function') || native.observerV2AbiVersion() !== 1) {
    throw new Error('This binary has no compatible observer-v2 extension.');
  }
  return { profile: 'observer-v2', client: await ObserverV2Client.create(native, peerId, options.createClientId()) };
}
