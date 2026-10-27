import path from 'node:path';
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { randomBytes, timingSafeEqual } from 'node:crypto';
import { publishPrivateEndpoint } from '../main/control-endpoint';
import type { RemoteService } from '../main/remote/service';
import type { RemoteViewPort } from '../main/remote/view';
import { failure } from '../shared/contracts';
import { requestEnvelopeSchema } from '../shared/schemas';
import { remoteEnvelopeSchema, remoteForwardRequestSchema, remoteRequestSchemas, responseSchemas } from '../shared/remote-schemas';
import { HOST_REQUEST_BYTES, HOST_RESPONSE_BYTES, HOST_STOP_TIMEOUT_MS, hostEndpointSchema, hostEnvelopeSchema, hostRequestSchemas, parseHostResponse } from '../shared/host-schemas';
import type { HostMode } from '../shared/host-contracts';
import { managerOperation, type ManagerContext, type HostSessionService } from './operations';
import { RpcClients, type RpcClient } from './rpc-clients';
import { getHostStatus } from './status';

export const hostEndpointPath = (userData: string): string => path.join(userData, 'host', 'endpoint.json');
export type HostRemoteService = Pick<RemoteService, 'getState' | 'configure' | 'login' | 'connect' | 'disconnect' | 'approve' | 'reject' | 'revoke' | 'forget' | 'request' | 'subscribe'> & Partial<Pick<RemoteService, 'createView'>>;
export interface HostRpcOptions extends Pick<ManagerContext, 'keepAwake' | 'ssh' | 'desktop'> {
  userData: string; mode: HostMode; service: HostSessionService; remote?: HostRemoteService | null;
  shutdown(): Promise<void>; startedAt?: string; guard?(): void; drainTimeoutMs?: number;
}
export interface HostRpc { close(): Promise<void>; waitForStopReplies(): Promise<void> }
async function readBody(request: IncomingMessage): Promise<unknown> {
  const chunks: Buffer[] = []; let bytes = 0;
  for await (const chunk of request) {
    if (!Buffer.isBuffer(chunk)) throw new Error('Invalid request encoding.');
    bytes += chunk.length; if (bytes > HOST_REQUEST_BYTES) throw new Error('Host request is too large.');
    chunks.push(chunk);
  }
  return JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(Buffer.concat(chunks)));
}
async function remoteOperation(remote: HostRemoteService | null | undefined, view: RemoteViewPort | undefined, method: string, payload: unknown): Promise<unknown> {
  const m = remoteEnvelopeSchema.parse({ method: method.slice(7), payload }).method;
  remoteRequestSchemas[m].parse(payload);
  if (!remote) return failure('UNSUPPORTED', 'Remote access is unavailable.');
  switch (m) {
    case 'getState': return remote.getState();
    case 'configure': return remote.configure(remoteRequestSchemas.configure.parse(payload));
    case 'login': return remote.login(remoteRequestSchemas.login.parse(payload));
    case 'connect': return remote.connect(remoteRequestSchemas.connect.parse(payload));
    case 'disconnect': return remote.disconnect(remoteRequestSchemas.disconnect.parse(payload));
    case 'approve': return remote.approve(remoteRequestSchemas.approve.parse(payload));
    case 'reject': return remote.reject(remoteRequestSchemas.reject.parse(payload));
    case 'revoke': return remote.revoke(remoteRequestSchemas.revoke.parse(payload));
    case 'forget': return remote.forget(remoteRequestSchemas.forget.parse(payload));
    case 'request': { const input = remoteForwardRequestSchema.parse(payload); return responseSchemas[input.method].parse(await (view ?? remote).request(input)); }
  }
}
export async function startHostRpc(options: HostRpcOptions): Promise<HostRpc> {
  const secret = randomBytes(32).toString('hex');
  const createView = options.remote?.createView?.bind(options.remote);
  const clients = new RpcClients(options.service, createView);
  const startedAt = options.startedAt ?? new Date().toISOString();
  const unsubscribes = [options.service.subscribe(event => clients.broadcast({ type: 'changed', event: { revision: event.revision, reason: event.reason } })), options.remote?.subscribe(event => { if (!options.remote?.createView || event.type === 'state') clients.broadcast({ type: 'remote', event }); }), options.keepAwake?.subscribe(event => clients.broadcast({ type: 'keepAwake', event }))];
  let closing = false, operations = 0, stopTask: Promise<void> | undefined;
  const sockets = new Set<IncomingMessage['socket']>();
  const stopResponses = new Set<IncomingMessage['socket']>();
  const stopReplies = new Set<Promise<void>>();
  async function waitForStopReplies(): Promise<void> {
    if (!stopReplies.size) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    await Promise.race([Promise.allSettled([...stopReplies]), new Promise<void>(resolve => { timer = setTimeout(resolve, 5000); timer.unref(); })]);
    if (timer) clearTimeout(timer);
  }
  const startStop = () => stopTask ??= Promise.resolve().then(() => options.shutdown()).catch(error => { stopTask = undefined; throw error; });
  const work = new Set<{ task: Promise<void>; drain: boolean }>();
  const server = createServer({ maxHeaderSize: 8192, requestTimeout: 30_000, headersTimeout: 10_000 }, (request, response) => {
    const record = { task: Promise.resolve(), drain: true };
    record.task = handle(request, response, record); work.add(record); void record.task.finally(() => work.delete(record));
  });
  server.maxConnections = 128; server.on('error', () => {});
  server.on('connection', socket => { sockets.add(socket); socket.once('close', () => { sockets.delete(socket); stopResponses.delete(socket); }); });
  async function handle(request: IncomingMessage, response: ServerResponse, operation: { drain: boolean }): Promise<void> {
    let requestId: string | undefined, client: RpcClient | undefined, countedClient = false;
    const abort = new AbortController(); response.on('close', () => abort.abort());
    const timedOut = () => { abort.abort(); request.destroy(); response.destroy(); };
    let deadline = setTimeout(timedOut, 30_000); deadline.unref();
    function reply(value: unknown, statusCode = 200): void {
      const text = JSON.stringify(value);
      if (Buffer.byteLength(text) > HOST_RESPONSE_BYTES) throw new Error('Host response is too large.');
      if (!response.destroyed) { response.writeHead(statusCode, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store', Connection: 'close' }); response.end(text); }
    }
    try {
      const auth = request.headers.authorization;
      if (closing || request.method !== 'POST' || request.url !== '/v1/request' || request.headers.origin || !auth || !/^Bearer [a-f0-9]{64}$/.test(auth) || !timingSafeEqual(Buffer.from(auth.slice(7)), Buffer.from(secret))) { response.writeHead(403); response.end(); return; }
      options.guard?.();
      if (operations >= 128) throw new Error('Host is busy.');
      operations++;
      try {
        const envelope = hostEnvelopeSchema.parse(await readBody(request)); requestId = envelope.requestId;
        const { method, payload } = envelope;
        // Parse payloads before creating client state or invoking any service method.
        hostRequestSchemas[method].parse(payload);
        if (method !== 'host.open') { client = clients.get(envelope.clientId); if (client.busy >= 8) throw new Error('Too many pending client operations.'); client.busy++; countedClient = true; }
        options.guard?.();
        let value: unknown;
        if (method === 'host.open') value = { clientId: clients.open().id };
        else if (method === 'host.status') value = getHostStatus(options.mode, startedAt, options.service, options.keepAwake, !!options.desktop);
        else if (method === 'host.poll' && client) value = await clients.poll(client, abort.signal);
        else if (method === 'host.close' && client) { clients.drop(client.id); value = { closed: true }; }
        else if (method === 'host.stop') {
          operation.drain = false;
          const input = hostRequestSchemas['host.stop'].parse(payload);
          if (input.wait) {
            stopResponses.add(request.socket);
            const finished = new Promise<void>(resolve => { response.once('finish', resolve); response.once('close', resolve); });
            stopReplies.add(finished);
            void finished.finally(() => { stopResponses.delete(request.socket); stopReplies.delete(finished); });
            clearTimeout(deadline); deadline = setTimeout(timedOut, HOST_STOP_TIMEOUT_MS); deadline.unref();
            await startStop(); value = { stopped: true };
          } else value = { stopping: true };
        }
        else if (method.startsWith('remote.')) value = await remoteOperation(options.remote, client?.remote, method, payload);
        else if (client) {
          // Update shutdown closes this listener from inside the request handler.
          // Waiting for that same handler here would deadlock the committed update.
          if (method === 'installUpdate') operation.drain = false;
          const manager = requestEnvelopeSchema.parse({ version: 1, method, payload });
          value = await managerOperation({ ...options, delivery: client.delivery, clientId: client.id }, manager.method, manager.payload);
        } else throw new Error('Host client is required.');
        reply({ requestId, ok: true, value: parseHostResponse(method, value) });
        if (method === 'host.stop' && !hostRequestSchemas['host.stop'].parse(payload).wait) {
          // Raw RPC callers receive an acceptance ACK. HostClient.stop waits for completion.
          setImmediate(() => { void startStop().catch(error => { console.error('Shellfox host stop failed.', error); }); });
        }
      } finally { operations--; }
    } catch (error) {
      try { reply({ requestId, ok: false, error: error instanceof Error ? error.message.slice(0, 1000) || 'Host request failed.' : 'Host request failed.' }, 400); }
      catch { response.destroy(); }
    } finally { if (client && countedClient) client.busy--; clearTimeout(deadline); }
  }
  try {
    await new Promise<void>((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', () => { server.removeListener('error', reject); resolve(); }); });
    const address = server.address(); if (!address || typeof address === 'string') throw new Error('Host listener unavailable.');
    const remove = await publishPrivateEndpoint(options.userData, hostEndpointPath(options.userData), hostEndpointSchema, { version: 1, host: '127.0.0.1', port: address.port, secret, mode: options.mode });
    let closed: Promise<void> | undefined;
    return { waitForStopReplies, close: () => closed ??= (async () => {
      closing = true; const clientCleanup = clients.dispose(); for (const unsubscribe of unsubscribes) unsubscribe?.();
      const stopped = new Promise<void>(resolve => server.close(() => resolve()));
      for (const socket of sockets) if (!stopResponses.has(socket)) socket.destroy();
      const drained = Promise.allSettled([...work].filter(operation => operation.drain).map(operation => operation.task));
      let timer: ReturnType<typeof setTimeout> | undefined;
      await Promise.race([drained, new Promise<void>(resolve => { timer = setTimeout(resolve, options.drainTimeoutMs ?? 30_000); timer.unref(); })]);
      if (timer) clearTimeout(timer);
      await clientCleanup;
      // Waiting stop requests receive their final reply after the caller finishes DB/ownership cleanup.
      if (!stopResponses.size) await stopped;
      await remove();
    })() };
  } catch (error) {
    await clients.dispose(); for (const unsubscribe of unsubscribes) unsubscribe?.(); server.closeAllConnections(); server.close(); throw error;
  }
}
