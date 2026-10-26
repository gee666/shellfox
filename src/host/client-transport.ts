import { request } from 'node:http';
import { randomUUID } from 'node:crypto';
import type { HostEndpoint, HostMethod, HostResponse } from '../shared/host-contracts';
import { HOST_REQUEST_BYTES, HOST_RESPONSE_BYTES, HOST_STOP_TIMEOUT_MS, hostReplySchema, hostRequestSchemas, parseHostResponse } from '../shared/host-schemas';

export class HostTransportError extends Error {}

export async function hostRequest<M extends HostMethod>(endpoint: HostEndpoint, clientId: string | undefined, method: M, payload: unknown, signal?: AbortSignal): Promise<HostResponse<M>> {
  const requestId = randomUUID();
  const body = JSON.stringify({ version: 1, requestId, clientId, method, payload: hostRequestSchemas[method].parse(payload) });
  if (Buffer.byteLength(body) > HOST_REQUEST_BYTES) throw new Error('Host request is too large.');
  const raw = await new Promise<unknown>((resolve, reject) => {
    const req = request({ host: endpoint.host, port: endpoint.port, path: '/v1/request', method: 'POST', headers: { Authorization: 'Bearer ' + endpoint.secret, 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(body), Connection: 'close' }, signal, agent: false }, response => {
      let bytes = 0; const chunks: Buffer[] = [];
      response.on('data', (chunk: Buffer) => { bytes += chunk.length; if (bytes > HOST_RESPONSE_BYTES) { req.destroy(new Error('Host response is too large.')); return; } chunks.push(chunk); });
      response.on('error', error => reject(new HostTransportError(error.message)));
      response.on('end', () => { try { resolve(JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(Buffer.concat(chunks)))); } catch { reject(new Error('Invalid host response.')); } });
    });
    const timeout = method === 'host.stop' && hostRequestSchemas['host.stop'].parse(payload).wait ? HOST_STOP_TIMEOUT_MS : 30_000;
    req.setTimeout(timeout, () => req.destroy(new Error('Host request timed out.')));
    req.on('error', error => reject(new HostTransportError(error.message))); req.end(body);
  });
  const reply = hostReplySchema.parse(raw);
  if (reply.requestId !== requestId) throw new Error('Host response requestId mismatch.');
  if (!reply.ok) throw new Error(reply.error);
  return parseHostResponse(method, reply.value);
}
