import type { RemoteClient } from './client';
export type { ClientState, RemoteClientOptions } from './client';

/** Public presentation contract, without the legacy client's private transport state. */
export type HostClient = Pick<RemoteClient, keyof RemoteClient>;
