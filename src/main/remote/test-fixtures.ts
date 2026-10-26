import { createHash, createHmac, randomUUID } from 'node:crypto';
import { createServer, type Server } from 'node:http';
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import path from 'node:path';
import { z } from 'zod';
import { EmbeddedSessionService } from '../terminal/service';
import { PtyBackend } from '../terminal/backend';
import { MemoryRepository, factoryFixture } from '../terminal/test-fixtures';
import type { HelperPort, HelperStatus } from './helper';
import { RemoteService } from './service';
import { RemoteStore } from './storage';
export const identity = { id: 'client-node', hostname: 'client' };
export function signedHeaders(secret: string, peer: { id: string; hostname: string }, requestPath: string, body: string, timestamp = String(Date.now())) {
  const encoded = Buffer.from(JSON.stringify(peer)).toString('base64url');
  const message = [timestamp, encoded, 'POST', requestPath, createHash('sha256').update(body).digest('hex')].join('\n');
  return { 'Content-Type': 'application/json', 'x-shellfox-peer': encoded, 'x-shellfox-timestamp': timestamp, 'x-shellfox-signature': createHmac('sha256', secret).update(message).digest('hex') };
}
export async function embeddedFixture() {
  const f = factoryFixture(), repository = new MemoryRepository();
  const backend = new PtyBackend(f.options);
  const service = new EmbeddedSessionService(repository, backend, () => ({ setWatch: () => {}, dispose: () => {} }));
  await service.initialize();
  return { ...f, repository, service, backend };
}
const startSchema = z.object({ backendUrl: z.url(), backendSecret: z.string(), proxySecret: z.string() });
const proxySchema = z.object({ peerId: z.string(), path: z.enum(['/v1/pair', '/v1/events', '/v1/request', '/v1/disconnect']), body: z.unknown() });
/** Exercises real HTTP and real session/delivery code; replaces only tsnet discovery and dialing. */
export class FakeRemoteHelper implements HelperPort {
  server?: Server;
  backendUrl = ''; secret = ''; proxySecret = '';
  readonly commands: { method: string; payload: unknown }[] = [];
  readonly requests: z.infer<typeof proxySchema>[] = [];
  private readonly statusListeners = new Set<(status: HelperStatus) => void>();
  subscribeStatus(listener: (status: HelperStatus) => void): () => void { this.statusListeners.add(listener); return () => { this.statusListeners.delete(listener); }; }
  emitStatus(status: HelperStatus): void { for (const listener of this.statusListeners) listener(status); }
  constructor(readonly id: string, private readonly network: Map<string, FakeRemoteHelper>) {}
  async command(method: string, payload: unknown): Promise<unknown> {
    this.commands.push({ method, payload });
    if (method === 'status' || method === 'login') return { status: 'online', loginUrl: null, error: null };
    if (method === 'peers') return { peers: [...this.network.values()].filter(peer => peer.id !== this.id).map(peer => ({ id: peer.id, hostname: peer.id, addresses: ['100.64.0.1'], online: true })) };
    if (method !== 'start') throw new Error('Unknown test helper command');
    const input = startSchema.parse(payload); this.backendUrl = input.backendUrl; this.secret = input.backendSecret; this.proxySecret = input.proxySecret;
    this.network.set(this.id, this);
    this.server = createServer((req, res) => {
      void (async () => {
        if (req.headers.authorization !== 'Bearer ' + this.proxySecret) { res.writeHead(401); res.end(); return; }
        const chunks: Buffer[] = []; for await (const chunk of req) chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
        const input = proxySchema.parse(JSON.parse(Buffer.concat(chunks).toString('utf8'))); this.requests.push(input);
        const host = this.network.get(input.peerId); if (!host) { res.writeHead(503); res.end(); return; }
        const body = JSON.stringify(input.body);
        const response = await fetch(host.backendUrl + input.path, { method: 'POST', body, headers: signedHeaders(host.secret, { id: this.id, hostname: this.id }, input.path, body) });
        res.writeHead(200, { 'Content-Type': 'application/json' }); res.end(await response.text());
      })().catch(() => { if (!res.destroyed) { res.writeHead(500); res.end(); } });
    });
    const server = this.server;
    await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
    const address = server.address(); if (!address || typeof address === 'string') throw new Error('Test proxy failed.');
    return { proxyUrl: `http://127.0.0.1:${address.port}` };
  }
  async dispose(): Promise<void> {
    this.network.delete(this.id); const server = this.server; this.server = undefined;
    if (server) { server.closeAllConnections(); await new Promise<void>(resolve => server.close(() => resolve())); }
  }
}
export async function remoteFixture(network: Map<string, FakeRemoteHelper>, id: string) {
  await mkdir('tmp', { recursive: true }); const directory = await mkdtemp(path.resolve('tmp/remote-test-'));
  const embedded = await embeddedFixture(), helper = new FakeRemoteHelper(id, network), store = new RemoteStore(directory, id);
  const remote = new RemoteService(directory, store, helper, embedded.service, id);
  await remote.initialize();
  await remote.configure({ enabled: true, hostname: id });
  return { ...embedded, remote, helper, store, directory, create: () => embedded.service.createSession({ cwd: '/work', requestId: randomUUID() }), dispose: async () => { await remote.dispose(); await embedded.service.dispose(); await rm(directory, { recursive: true, force: true }); } };
}
