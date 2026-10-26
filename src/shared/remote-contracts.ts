import type { z } from 'zod';
import type { Result } from './contracts';
import type { requestSchemas, responseSchemas } from './schemas';
import type { remoteMethodSchema, remoteStateSchema, remotePeerSchema, remoteApprovalSchema, remoteEventSchema, remoteHostEventSchema } from './remote-schemas';
export type RemoteMethod = z.infer<typeof remoteMethodSchema>;
export type RemoteRequestPayload<M extends RemoteMethod> = z.input<(typeof requestSchemas)[M]>;
export type RemoteResponse<M extends RemoteMethod> = z.output<(typeof responseSchemas)[M]>;
export type RemoteState = z.infer<typeof remoteStateSchema>;
export type RemotePeer = z.infer<typeof remotePeerSchema>;
export type RemoteApproval = z.infer<typeof remoteApprovalSchema>;
export type RemoteEvent = z.infer<typeof remoteEventSchema>;
export type RemoteHostEvent = z.infer<typeof remoteHostEventSchema>;
export interface RemoteApi {
  getState(): Promise<Result<RemoteState>>;
  configure(input: { enabled: boolean; hostname: string }): Promise<Result<RemoteState>>;
  login(input?: { authKey?: string }): Promise<Result<RemoteState>>;
  connect(input: { peerId: string }): Promise<Result<RemoteState>>;
  disconnect(input: { peerId: string }): Promise<Result<RemoteState>>;
  approve(input: { peerId: string }): Promise<Result<RemoteState>>;
  reject(input: { peerId: string }): Promise<Result<RemoteState>>;
  revoke(input: { peerId: string }): Promise<Result<RemoteState>>;
  forget(input: { peerId: string }): Promise<Result<RemoteState>>;
  request<M extends RemoteMethod>(input: { peerId: string; method: M; payload: RemoteRequestPayload<M> }): Promise<RemoteResponse<M>>;
  subscribe(listener: (event: RemoteEvent) => void): () => void;
}
declare global { interface Window { shellfoxRemote?: RemoteApi } }
