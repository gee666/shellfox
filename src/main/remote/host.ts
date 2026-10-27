import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { z } from 'zod';
import { failure, success, type AppError } from '../../shared/contracts';
import type { RemoteHostEvent } from '../../shared/remote-contracts';
import { REMOTE_REQUEST_BYTES } from '../../shared/remote-schemas';
import type { EmbeddedSessionService } from '../terminal/service';
import type { TerminalControlView } from '../terminal/control';
import { verifyPeer, type peerIdentitySchema } from './auth';
import { remoteOperation } from './operations';
const identity = z.object({ clientId: z.uuid() }).strict();
const operation = identity.extend({ method: z.string().max(100), payload: z.unknown() });
interface Client {
  peerId: string; delivery: TerminalControlView; events: RemoteHostEvent[]; bytes: number; touched: number;
  notify?: () => void; pendingPoll: boolean; requests: number;
}
export class RemoteHost {
  private readonly server = createServer((request, response) => { void this.handle(request, response); });
  private readonly clients = new Map<string, Client>();
  private readonly rates = new Map<string, { since: number; count: number }>();
  private readonly denied = new Map<string, number>();
  private readonly unsubscribe: () => void;
  private readonly sweep: ReturnType<typeof setInterval>;
  constructor(private readonly service: EmbeddedSessionService, private readonly secret: string, private readonly approved: (peerId: string) => boolean, private readonly pending: (peer: z.infer<typeof peerIdentitySchema>, clientId: string) => void, private readonly authorize?: (peer: z.infer<typeof peerIdentitySchema>, clientId: string) => AppError | null, private readonly withdraw?: (peer: z.infer<typeof peerIdentitySchema>, clientId: string) => void) {
    this.server.requestTimeout = 30_000; this.server.headersTimeout = 10_000; this.server.maxConnections = 64;
    // Host-local CLI focus requests must never redirect a remote client's input.
    this.unsubscribe = service.subscribe(({ revision, reason }) => { for (const [id] of this.clients) this.enqueue(id, { type: 'changed', event: { revision, reason } }); });
    this.sweep = setInterval(() => {
      for (const [id, client] of this.clients) if (Date.now() - client.touched > 60_000 || !this.approved(client.peerId)) this.drop(id);
      for (const [id, rate] of this.rates) if (Date.now() - rate.since > 60_000) this.rates.delete(id);
      for (const [id, until] of this.denied) if (until <= Date.now()) this.denied.delete(id);
    }, 10_000); this.sweep.unref();
  }
  async listen(): Promise<string> {
    await new Promise<void>((resolve, reject) => { this.server.once('error', reject); this.server.listen(0, '127.0.0.1', resolve); });
    const address = this.server.address(); if (!address || typeof address === 'string') throw new Error('Remote listener failed.');
    return `http://127.0.0.1:${address.port}`;
  }
  private block(id: string): void {
    this.denied.delete(id); this.denied.set(id, Date.now() + 300_000);
    while (this.denied.size > 8192) { const oldest = this.denied.keys().next().value; if (oldest === undefined) break; this.denied.delete(oldest); }
  }
  private isDenied(id: string): boolean {
    const until = this.denied.get(id); if (until === undefined) return false;
    if (until > Date.now()) return true;
    this.denied.delete(id); return false;
  }
  revoke(peerId: string): void { for (const [id, client] of this.clients) if (client.peerId === peerId) { this.block(id); this.drop(id); } }
  private drop(id: string): void { const client = this.clients.get(id); if (!client) return; this.clients.delete(id); client.delivery.dispose(); this.service.clearRemoteVisibleSessions(id); client.notify?.(); }
  private purge(id: string, tabId: string): void {
    const client = this.clients.get(id); if (!client) return;
    client.events = client.events.filter(event => event.type !== 'terminal' || event.event.tabId !== tabId);
    client.bytes = client.events.reduce((sum, event) => sum + Buffer.byteLength(JSON.stringify(event)), 0);
  }
  private enqueue(id: string, event: RemoteHostEvent): void {
    const client = this.clients.get(id); if (!client) return;
    // Metadata changes coalesce, terminal credit and queue limits bound each viewer.
    if (event.type === 'changed') {
      for (const previous of client.events) if (previous.type === 'changed') client.bytes -= Buffer.byteLength(JSON.stringify(previous));
      client.events = client.events.filter(e => e.type !== 'changed');
    }
    const bytes = Buffer.byteLength(JSON.stringify(event));
    if (client.events.length >= 1024 || client.bytes + bytes > 2 * 1024 * 1024) { this.drop(id); return; }
    client.events.push(event); client.bytes += bytes; client.notify?.();
  }
  private reply(response: ServerResponse, value: unknown): void {
    if (!response.destroyed && !response.writableEnded) { response.writeHead(200, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' }); response.end(JSON.stringify(value)); }
  }
  private async read(request: IncomingMessage): Promise<Buffer> {
    const chunks: Buffer[] = []; let size = 0;
    for await (const chunk of request) { const data: Buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk); size += data.length; if (size > REMOTE_REQUEST_BYTES) throw new Error('Remote request exceeds the byte limit.'); chunks.push(data); }
    return Buffer.concat(chunks);
  }
  private async handle(request: IncomingMessage, response: ServerResponse): Promise<void> {
    try {
      const url = request.url ?? '';
      if (request.method !== 'POST' || !['/v1/discover', '/v1/pair', '/v1/request', '/v1/events', '/v1/disconnect'].includes(url)) { response.writeHead(404); response.end(); return; }
      const body = await this.read(request), peer = verifyPeer(this.secret, request.headers, request.method, url, body);
      if (!peer) { this.reply(response, failure('AUTH_FAILED', 'Unverified remote peer.')); return; }
      let rate = this.rates.get(peer.id);
      if (!rate || Date.now() - rate.since > 1000) { rate = { since: Date.now(), count: 0 }; this.rates.set(peer.id, rate); }
      if (++rate.count > 240 || this.rates.size > 2000) { this.reply(response, failure('VALIDATION', 'Remote request rate exceeded.', true)); return; }
      if (url === '/v1/discover') {
        this.reply(response, success({ service: 'shellfox', protocolVersion: 1 })); return;
      }
      const raw: unknown = JSON.parse(body.toString('utf8'));
      const input = url === '/v1/request' ? operation.parse(raw) : identity.parse(raw);
      const id = peer.id + '/' + input.clientId;
      if (url === '/v1/disconnect') {
        this.withdraw?.(peer, input.clientId); this.block(id); this.drop(id);
        this.reply(response, success({ disconnected: true })); return;
      }
      if (this.isDenied(id)) { this.reply(response, failure('ACCESS_DENIED', 'This connection was declined or revoked. Request access again.', false)); return; }
      const permission = this.authorize?.(peer, input.clientId);
      if (permission) { this.reply(response, { ok: false, error: permission }); return; }
      if (!this.approved(peer.id)) { this.pending(peer, input.clientId); this.reply(response, failure('APPROVAL_REQUIRED', 'Approve this device in Shellfox on the host first.', true)); return; }
      if (url === '/v1/pair') {
        const existing = this.clients.get(id);
        if (existing) { existing.touched = Date.now(); existing.notify?.(); }
        if (!existing) {
          if (this.clients.size >= 32) { this.reply(response, failure('VALIDATION', 'Too many remote clients.')); return; }
          this.clients.set(id, { peerId: peer.id, delivery: this.service.createTerminalView(event => this.enqueue(id, { type: 'terminal', event }), tabId => this.purge(id, tabId)), events: [], bytes: 0, touched: Date.now(), pendingPoll: false, requests: 0 });
        }
        this.reply(response, success({ paired: true, terminalControl: 'exclusive-v1' })); return;
      }
      const client = this.clients.get(id);
      if (!client) { this.reply(response, failure('AUTH_FAILED', 'Reconnect this remote view.', true)); return; }
      client.touched = Date.now();
      if (url === '/v1/events') {
        if (client.pendingPoll) { this.reply(response, failure('VALIDATION', 'A poll is already pending.')); return; }
        client.pendingPoll = true;
        if (!client.events.length) await new Promise<void>(resolve => {
          const done = () => { clearTimeout(timer); response.removeListener('close', done); client.notify = undefined; resolve(); };
          const timer = setTimeout(done, 20_000); client.notify = done; response.once('close', done);
        });
        client.pendingPoll = false;
        if (!this.approved(peer.id) || this.isDenied(id)) { this.reply(response, failure('ACCESS_DENIED', 'The host revoked this connection. Request access again.', false)); return; }
        if (this.clients.get(id) !== client) { this.reply(response, failure('AUTH_FAILED', 'Remote view disconnected.', true)); return; }
        const events = client.events; client.events = []; client.bytes = 0;
        this.reply(response, success({ events })); return;
      }
      if (client.requests >= 8) { this.reply(response, failure('VALIDATION', 'Too many pending operations.', true)); return; }
      const command = operation.parse(raw); client.requests++;
      try {
        const result = await remoteOperation(this.service, client.delivery, id, command.method, command.payload);
        const permission = this.authorize?.(peer, input.clientId);
        if (!this.approved(peer.id) || this.isDenied(id) || permission?.code === 'ACCESS_DENIED') this.reply(response, failure('ACCESS_DENIED', 'The host revoked this connection.', false));
        else this.reply(response, result);
      }
      finally { client.requests--; }
    } catch { this.reply(response, failure('VALIDATION', 'Remote request could not be completed.', true)); }
  }
  async dispose(): Promise<void> {
    clearInterval(this.sweep); this.unsubscribe(); for (const id of this.clients.keys()) this.drop(id);
    this.server.closeAllConnections(); await new Promise<void>(resolve => this.server.close(() => resolve()));
  }
}
