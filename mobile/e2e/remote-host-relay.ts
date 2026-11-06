import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import path from 'node:path';
import { realpath } from 'node:fs/promises';
import { z } from 'zod';
import { REMOTE_REQUEST_BYTES } from '../../src/shared/remote-schemas';

export interface QaRequest { at: string; route: string; method?: string; path?: string; bytes: number; result?: string }
export interface QaRemoteConfiguration {
  protocol: 1; hostId: string; clientPeer: { id: string; hostname: string }; hmacSecret: string; root: string; files: string[];
}
const controlSchema = z.object({ action: z.enum(['approve', 'revoke', 'shutdown']) }).strict();
const routes = new Set(['/v1/discover', '/v1/pair', '/v1/request', '/v1/events', '/v1/disconnect']);
const pathMethods = new Set(['listFiles', 'readFile', 'readFileAsset', 'writeFile', 'writeImageFile', 'beginFileRead', 'beginFileWrite', 'resolveDirectory', 'completeDirectory']);
const harmlessMethods = new Set(['getSnapshot', 'getTerminalProfiles', 'getHomeDirectory', 'setVisibleSessions', 'readFileChunk', 'writeFileChunk', 'finishFileWrite', 'cancelFileTransfer']);
async function body(request: IncomingMessage): Promise<Buffer> {
  const parts: Buffer[] = []; let size = 0;
  for await (const part of request) { const bytes = Buffer.isBuffer(part) ? part : Buffer.from(part); size += bytes.length; if (size > REMOTE_REQUEST_BYTES) throw new Error('QA body exceeds production request limit.'); parts.push(bytes); }
  return Buffer.concat(parts);
}
function reply(response: ServerResponse, value: unknown, status = 200) {
  if (response.destroyed || response.writableEnded) return;
  response.writeHead(status, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' }); response.end(JSON.stringify(value));
}
const within = (root: string, candidate: string): boolean => { const relative = path.relative(root, candidate); return relative !== '..' && !relative.startsWith('..' + path.sep) && !path.isAbsolute(relative); };
async function confined(root: string, candidate: string): Promise<boolean> {
  if (!path.isAbsolute(candidate) || !within(root, path.resolve(candidate)) || await realpath(root) !== root) return false;
  const target = path.resolve(candidate); let ancestor = target;
  for (;;) {
    try { return within(root, path.resolve(await realpath(ancestor), path.relative(ancestor, target))); }
    catch (error) {
      if (!(error instanceof Error && 'code' in error && error.code === 'ENOENT')) return false;
      const parent = path.dirname(ancestor); if (parent === ancestor) return false; ancestor = parent;
    }
  }
}
/** QA-only loopback adapter. Production RemoteHost still authenticates every signed byte.
 * No application, user identity, live tailnet credentials or production transport settings are changed. */
export async function startQaRemoteRelay(options: {
  port: number; backendUrl: string; configuration: QaRemoteConfiguration; controlToken: string;
  status: () => Promise<unknown>; control: (action: z.infer<typeof controlSchema>['action']) => Promise<void>;
  record: (request: QaRequest) => void;
}) {
  options.configuration.root = await realpath(options.configuration.root);
  const stopping = new AbortController();
  const server = createServer((request, response) => {
    void (async () => {
      const route = request.url ?? '';
      if (request.method === 'GET' && route === '/__qa/config') { reply(response, options.configuration); return; }
      if (route.startsWith('/__qa/')) {
        if (request.headers.authorization !== 'Bearer ' + options.controlToken) { reply(response, { error: 'Host-runner authorization required.' }, 403); return; }
        if (route === '/__qa/status' && request.method === 'GET') { reply(response, await options.status()); return; }
        if (route === '/__qa/control' && request.method === 'POST') {
          const input = controlSchema.parse(JSON.parse((await body(request)).toString('utf8')));
          await options.control(input.action); reply(response, { ok: true, action: input.action }); return;
        }
        reply(response, { error: 'Unknown QA control.' }, 404); return;
      }
      if (request.method !== 'POST' || !routes.has(route)) { reply(response, { error: 'Unknown route.' }, 404); return; }
      const bytes = await body(request), audit: QaRequest = { at: new Date().toISOString(), route, bytes: bytes.length };
      let peer: unknown;
      try { const encoded = request.headers['x-shellfox-peer']; if (typeof encoded === 'string') peer = JSON.parse(Buffer.from(encoded, 'base64url').toString('utf8')); } catch { /* The real host also validates the signed identity. */ }
      if (!peer || typeof peer !== 'object' || !('id' in peer) || peer.id !== options.configuration.clientPeer.id || !('hostname' in peer) || peer.hostname !== options.configuration.clientPeer.hostname) {
        audit.result = 'QA_PEER_DENIED'; options.record(audit);
        reply(response, { ok: false, error: { code: 'AUTH_FAILED', message: 'Only the isolated QA Android peer is allowed.', retryable: false } }); return;
      }
      if (route === '/v1/request') {
        const raw: unknown = JSON.parse(bytes.toString('utf8'));
        if (!raw || typeof raw !== 'object' || !('method' in raw) || typeof raw.method !== 'string') throw new Error('Invalid QA request.');
        audit.method = raw.method;
        if (pathMethods.has(raw.method)) {
          if (!('payload' in raw) || !raw.payload || typeof raw.payload !== 'object' || !('path' in raw.payload) || typeof raw.payload.path !== 'string' || !await confined(options.configuration.root, raw.payload.path)) {
            audit.result = 'QA_ROOT_DENIED'; options.record(audit);
            reply(response, { ok: false, error: { code: 'ACCESS_DENIED', message: 'This isolated QA host exposes only its test-owned Windows directory.', retryable: false } }); return;
          }
          audit.path = raw.payload.path;
        } else if (!harmlessMethods.has(raw.method)) {
          audit.result = 'QA_METHOD_DENIED'; options.record(audit);
          reply(response, { ok: false, error: { code: 'UNSUPPORTED', message: 'This isolated QA host is for filesystem verification only.', retryable: false } }); return;
        }
      }
      const headers: Record<string, string> = { 'Content-Type': 'application/json' };
      for (const key of ['x-shellfox-peer', 'x-shellfox-timestamp', 'x-shellfox-signature']) { const value = request.headers[key]; if (typeof value === 'string') headers[key] = value; }
      const forwarded = await fetch(options.backendUrl + route, { method: 'POST', headers, body: new Uint8Array(bytes).buffer, signal: stopping.signal });
      const text = await forwarded.text();
      if (Buffer.byteLength(text) > 8 * 1024 * 1024) throw new Error('QA response exceeds production response limit.');
      const result: unknown = JSON.parse(text);
      audit.result = result && typeof result === 'object' && 'ok' in result && result.ok === true ? 'ok'
        : result && typeof result === 'object' && 'error' in result && result.error && typeof result.error === 'object' && 'code' in result.error ? String(result.error.code) : 'unknown';
      options.record(audit);
      if (!response.destroyed && !response.writableEnded) { response.writeHead(forwarded.status, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' }); response.end(text); }
    })().catch(error => reply(response, { error: error instanceof Error ? error.message : 'QA host request failed.' }, 400));
  });
  server.requestTimeout = 35000; server.headersTimeout = 10000; server.maxConnections = 64;
  await new Promise<void>((resolve, reject) => { server.once('error', reject); server.listen(options.port, '127.0.0.1', resolve); });
  const address = server.address(); if (!address || typeof address === 'string') throw new Error('QA relay address unavailable.');
  return { url: `http://127.0.0.1:${address.port}`, close: async () => { stopping.abort(); server.closeAllConnections(); await new Promise<void>(resolve => server.close(() => resolve())); } };
}
