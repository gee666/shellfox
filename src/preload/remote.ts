import { ipcRenderer } from 'electron';
import { failure } from '../shared/contracts';
import type { RemoteApi, RemoteMethod, RemoteRequestPayload, RemoteResponse } from '../shared/remote-contracts';
import { remoteEventSchema, remoteRequestSchemas, remoteResponseSchemas, responseSchemas } from '../shared/remote-schemas';

async function invoke(method: keyof typeof remoteRequestSchemas, payload: unknown): Promise<unknown> {
  if (!remoteRequestSchemas[method].safeParse(payload).success) return failure('VALIDATION', 'Invalid remote request.');
  try { return await ipcRenderer.invoke('remote:request', { method, payload }); }
  catch { return failure('INTERNAL', 'Remote request failed.', true); }
}
async function stateRequest(method: keyof typeof remoteResponseSchemas, payload: unknown) {
  const parsed = remoteResponseSchemas[method].safeParse(await invoke(method, payload));
  return parsed.success ? parsed.data : failure('INTERNAL', 'Invalid remote response.');
}
function request<M extends RemoteMethod>(input: { peerId: string; method: M; payload: RemoteRequestPayload<M> }): Promise<RemoteResponse<M>>;
async function request(input: { peerId: string; method: RemoteMethod; payload: RemoteRequestPayload<RemoteMethod> }): Promise<RemoteResponse<RemoteMethod>> {
  const parsed = responseSchemas[input.method].safeParse(await invoke('request', input));
  return parsed.success ? parsed.data : failure('INTERNAL', 'Invalid remote response.');
}
export const remoteApi: RemoteApi = {
  getState: () => stateRequest('getState', {}),
  configure: input => stateRequest('configure', input),
  login: input => stateRequest('login', input ?? {}),
  connect: input => stateRequest('connect', input),
  disconnect: input => stateRequest('disconnect', input),
  approve: input => stateRequest('approve', input),
  reject: input => stateRequest('reject', input),
  revoke: input => stateRequest('revoke', input),
  forget: input => stateRequest('forget', input),
  request,
  subscribe(listener) {
    if (typeof listener !== 'function') return () => {};
    const receive = (_event: unknown, raw: unknown) => { const parsed = remoteEventSchema.safeParse(raw); if (parsed.success) listener(parsed.data); };
    ipcRenderer.on('remote:event', receive);
    return () => ipcRenderer.removeListener('remote:event', receive);
  },
};
