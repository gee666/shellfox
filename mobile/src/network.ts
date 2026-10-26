import { NativeModules } from 'react-native';
import { z } from 'zod';
import { remoteNetworkPeerSchema, remoteStatusSchema } from '../../src/shared/remote-schemas';

const nativeSchema = z.object({
  start: z.custom<(hostname: string) => Promise<string>>((value) => typeof value === 'function'),
  status: z.custom<() => Promise<string>>((value) => typeof value === 'function'),
  peers: z.custom<() => Promise<string>>((value) => typeof value === 'function'),
  hosts: z.custom<() => Promise<string>>((value) => typeof value === 'function'),
  login: z.custom<(authKey: string) => Promise<string>>((value) => typeof value === 'function'),
  request: z.custom<(peerId: string, path: string, body: string) => Promise<string>>(
    (value) => typeof value === 'function',
  ),
  stop: z.custom<() => Promise<void>>((value) => typeof value === 'function'),
  logout: z.custom<() => Promise<void>>((value) => typeof value === 'function'),
});
export const nativeNetwork = nativeSchema.parse(NativeModules.ShellfoxNetwork);
export type NetworkPeer = z.infer<typeof remoteNetworkPeerSchema>;
export type NetworkStatus = z.infer<typeof remoteStatusSchema>;
export async function networkStatus(): Promise<NetworkStatus> {
  return remoteStatusSchema.parse(JSON.parse(await nativeNetwork.status()));
}
// Returns verified cached hosts immediately; native discovery runs in the background.
// Repeated calls pick up new hosts without probing ordinary peers every UI poll.
export interface DiscoveredHosts { peers: NetworkPeer[]; discovering: boolean }
export async function discoverHosts(): Promise<DiscoveredHosts> {
  return z
    .object({ peers: z.array(remoteNetworkPeerSchema).max(1000), discovering: z.boolean() }).strict()
    .parse(JSON.parse(await nativeNetwork.hosts()));
}
export async function networkPeers(): Promise<NetworkPeer[]> {
  return (await discoverHosts()).peers;
}
