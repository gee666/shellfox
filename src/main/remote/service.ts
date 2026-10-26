import { randomBytes, randomUUID } from 'node:crypto';
import path from 'node:path';
import { z } from 'zod';
import { failure, success, type AppError, type Result } from '../../shared/contracts';
import type { RemoteEvent, RemoteState, RemotePeer } from '../../shared/remote-contracts';
import { remoteForwardRequestSchema, remoteNetworkPeerSchema, remotePollResponseSchema, remoteStatusSchema, responseSchemas } from '../../shared/remote-schemas';
import type { EmbeddedSessionService } from '../terminal/service';
import type { HelperPort } from './helper';
import { RemoteStore, type RemoteStoredState } from './storage';
import { RemoteHost } from './host';
import { proxyRequest, validateProxyUrl } from './transport';
import { RemoteView, type RemoteViewPort } from './view';
import { RemoteApprovals } from './approvals';
import { RemoteAccessError, requirePair, remoteFailure, waitForRemoteRetry } from './pairing';
interface Connection { clientId: string; abort: AbortController; status: RemotePeer['connection']; lastError: AppError | null; pending: number; cycle?: AbortController }
export class RemoteService {
  private stored: RemoteStoredState;
  private status: Pick<RemoteState, 'status' | 'loginUrl' | 'error'> = { status: 'disabled', loginUrl: null, error: null };
  private peers: z.infer<typeof remoteNetworkPeerSchema>[] = [];
  private readonly approvals = new RemoteApprovals(id => this.stored.approvals.some(approval => approval.peerId === id), () => this.emitState());
  private readonly listeners = new Set<(event: RemoteEvent) => void>();
  private readonly connections = new Map<string, Connection>();
  private host?: RemoteHost;
  private proxyUrl?: string;
  private readonly proxySecret = randomBytes(32).toString('hex');
  private poll?: ReturnType<typeof setInterval>;
  private refreshing = false;
  private peersRefreshing = false;
  private statusRevision = 0;
  private unsubscribeStatus?: () => void;
  private networkEpoch = 0;
  private changing = false;
  private mutations: Promise<unknown> = Promise.resolve();
  private serialize<T>(action: () => Promise<T>): Promise<T> {
    const next = this.mutations.catch(() => {}).then(action);
    this.mutations = next;
    return next;
  }
  configure(input: { enabled: boolean; hostname: string }) { return this.serialize(() => this.configureNow(input)); }
  login(input: { authKey?: string }) { return this.serialize(() => this.loginNow(input)); }
  approve(input: { peerId: string }) { return this.serialize(() => this.approveNow(input)); }
  reject(input: { peerId: string }) { return this.serialize(() => this.rejectNow(input)); }
  revoke(input: { peerId: string }) { return this.serialize(() => this.revokeNow(input)); }
  connect(input: { peerId: string }) { return this.serialize(() => this.connectNow(input)); }
  disconnect(input: { peerId: string }) { return this.serialize(() => this.disconnectNow(input)); }
  forget(input: { peerId: string }) { return this.serialize(async () => {
    const next = { ...this.stored, peers: this.stored.peers.filter(peer => peer.id !== input.peerId) };
    try { await this.store.save(next); this.stored = next; return await this.disconnectNow(input); }
    catch { return failure('STORAGE_FAILED', 'Remote device could not be forgotten.'); }
  }); }
  private readonly views = new Map<string, RemoteViewPort>();
  createView(viewId: string): RemoteViewPort {
    const existing = this.views.get(viewId); if (existing) return existing;
    if (this.disposed || this.views.size >= 64) throw new Error('Remote view limit reached or service stopped.');
    const view = new RemoteView(peerId => this.connections.get(peerId), (peerId, path, body, signal) => this.send(peerId, path, body, signal), () => { this.views.delete(viewId); }, (peerId, error) => this.accessFailure(peerId, error, undefined, true));
    this.views.set(viewId, view); return view;
  }
  private disposed = false;
  constructor(private readonly directory: string, private readonly store: RemoteStore, private readonly helper: HelperPort, private readonly service: EmbeddedSessionService, hostname: string) {
    this.stored = { version: 1, enabled: false, hostname, approvals: [], peers: [] };
  }
  async initialize(): Promise<void> {
    try { this.stored = await this.store.load(); if (this.stored.enabled) await this.start(); }
    catch { this.status = { status: 'error', loginUrl: null, error: 'Remote access could not start. Check the Tailscale helper and saved preferences.' }; }
    this.emitState();
  }
  getState(): Result<RemoteState> {
    const known = new Map(this.stored.peers.map(peer => [peer.id, { ...peer, online: false }]));
    for (const peer of this.peers) known.set(peer.id, peer);
    return success({ ...this.status, enabled: this.stored.enabled, hostname: this.stored.hostname,
      peers: [...known.values()].map(peer => ({ ...peer, connection: this.connections.get(peer.id)?.status ?? 'disconnected' as const, lastError: this.connections.get(peer.id)?.lastError ?? null, approved: this.stored.approvals.some(a => a.peerId === peer.id), remembered: this.stored.peers.some(saved => saved.id === peer.id) })),
      approvals: this.stored.approvals, pending: [...this.approvals.pending.values()] });
  }
  subscribe(listener: (event: RemoteEvent) => void): () => void { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; }
  private emit(event: RemoteEvent): void { for (const listener of this.listeners) { try { listener(event); } catch { /* A view cannot break the network lifecycle. */ } } }
  private emitState(): void { const state = this.getState(); if (state.ok) this.emit({ type: 'state', state: state.value }); }
  private async start(authKey?: string): Promise<void> {
    if (this.disposed) return;
    this.status = { status: 'starting', loginUrl: null, error: null }; this.emitState();
    const epoch = this.networkEpoch;
    this.unsubscribeStatus = this.helper.subscribeStatus?.(raw => {
      const parsed = remoteStatusSchema.safeParse(raw);
      if (!parsed.success || epoch !== this.networkEpoch || this.disposed || !this.stored.enabled) return;
      this.statusRevision++; this.applyStatus(parsed.data); if (parsed.data.status === 'online') void this.refreshPeers();
    });
    const backendSecret = randomBytes(32).toString('hex');
    this.host = new RemoteHost(this.service, backendSecret, id => this.stored.approvals.some(a => a.peerId === id), () => {}, (peer, clientId) => this.approvals.authorize(peer, clientId), (peer, clientId) => this.approvals.withdraw(peer.id, clientId));
    const backendUrl = await this.host.listen();
    try {
      const result = await this.helper.command('start', { stateDirectory: path.join(this.directory, 'tailscale'), hostname: this.stored.hostname, ...(authKey ? { authKey } : {}), backendUrl, backendSecret, proxySecret: this.proxySecret });
      this.proxyUrl = validateProxyUrl(result);
      await this.refresh();
      this.poll = setInterval(() => { this.approvals.expire(); void this.refresh(); }, 5000); this.poll.unref();
    } catch (error) { await this.stop(); throw error; }
  }
  private applyStatus(status: Pick<RemoteState, 'status' | 'loginUrl' | 'error'>): void {
    this.status = { ...status, loginUrl: status.status === 'login-required' ? status.loginUrl : null };
    if (status.status !== 'online') this.peers = [];
    this.emitState();
  }
  private async refreshPeers(): Promise<void> {
    if (this.peersRefreshing || this.disposed || !this.proxyUrl) return;
    const epoch = this.networkEpoch, revision = this.statusRevision; this.peersRefreshing = true;
    try {
      const result = z.object({ peers: z.array(remoteNetworkPeerSchema).max(1000) }).strict().parse(await this.helper.command('peers', {}));
      if (epoch === this.networkEpoch && revision === this.statusRevision && !this.disposed && this.proxyUrl && this.status.status === 'online') { this.peers = result.peers; this.emitState(); }
    } catch { /* A peer refresh must not replace a fresh login status event. */ }
    finally {
      this.peersRefreshing = false;
      if (epoch === this.networkEpoch && revision !== this.statusRevision && !this.disposed && this.status.status === 'online') void this.refreshPeers();
    }
  }
  private async refresh(): Promise<void> {
    if (this.refreshing || this.disposed || !this.proxyUrl) return;
    this.refreshing = true;
    const epoch = this.networkEpoch, revision = this.statusRevision;
    try {
      const status = remoteStatusSchema.parse(await this.helper.command('status', {}));
      if (epoch !== this.networkEpoch || this.disposed || !this.proxyUrl) return;
      if (revision === this.statusRevision) this.applyStatus(status);
      await this.refreshPeers();
    } catch {
      if (epoch === this.networkEpoch && revision === this.statusRevision && !this.disposed) { this.status = { status: 'error', loginUrl: null, error: 'Tailscale is unavailable. Disable and re-enable remote access to restart it.' }; this.peers = []; }
    } finally { this.refreshing = false; if (epoch === this.networkEpoch && !this.disposed) this.emitState(); }
  }
  private async configureNow(input: { enabled: boolean; hostname: string }) {
    if (this.disposed) return failure('NATIVE_UNAVAILABLE', 'Shellfox is shutting down.');
    if (this.changing) return failure('INTERNAL', 'Remote configuration is already changing.', true);
    this.changing = true;
    try {
      await this.stop(); this.stored = { ...this.stored, ...input }; await this.store.save(this.stored);
      if (input.enabled) await this.start();
      this.emitState(); return this.getState();
    } catch { this.status = { status: 'error', loginUrl: null, error: 'Remote access could not be configured.' }; this.emitState(); return failure('INTERNAL', 'Remote access could not be configured.', true); }
    finally { this.changing = false; }
  }
  private async loginNow(input: { authKey?: string }) {
    if (this.disposed) return failure('NATIVE_UNAVAILABLE', 'Shellfox is shutting down.');
    if (!this.stored.enabled) return failure('UNSUPPORTED', 'Enable remote access first.');
    try { await this.helper.command('login', input); await this.refresh(); return this.getState(); }
    catch { return failure('AUTH_FAILED', 'Tailscale login could not start.', true); }
  }
  private async approveNow(input: { peerId: string }) {
    const peer = this.approvals.pending.get(input.peerId);
    if (!peer) return failure('NOT_FOUND', 'No pairing request from this device.');
    if (this.stored.approvals.length >= 1000) return failure('VALIDATION', 'The approved device limit is reached.');
    const next = { ...this.stored, approvals: [...this.stored.approvals.filter(a => a.peerId !== peer.id), { peerId: peer.id, hostname: peer.hostname, approvedAt: new Date().toISOString() }] };
    try { await this.store.save(next); this.stored = next; this.approvals.approve(peer.id); this.emitState(); return this.getState(); }
    catch { return failure('STORAGE_FAILED', 'Approval could not be saved.'); }
  }
  private async rejectNow(input: { peerId: string }) {
    if (!this.approvals.pending.has(input.peerId)) return failure('NOT_FOUND', 'No pending request from this device.');
    this.approvals.deny(input.peerId); this.host?.revoke(input.peerId); this.emitState(); return this.getState();
  }
  private async revokeNow(input: { peerId: string }) {
    const next = { ...this.stored, approvals: this.stored.approvals.filter(a => a.peerId !== input.peerId) };
    try {
      await this.store.save(next); this.stored = next;
      this.approvals.deny(input.peerId);
      this.host?.revoke(input.peerId); this.emitState(); return this.getState();
    }
    catch { return failure('STORAGE_FAILED', 'Revocation could not be saved.'); }
  }
  private async connectNow(input: { peerId: string }) {
    if (this.disposed) return failure('NATIVE_UNAVAILABLE', 'Shellfox is shutting down.');
    if (!this.proxyUrl || !this.stored.enabled) return failure('UNSUPPORTED', 'Remote access is not running.');
    const previous = this.connections.get(input.peerId);
    if (previous?.status === 'rejected') await this.disconnectNow(input);
    else if (previous) return this.getState();
    const peer = this.peers.find(p => p.id === input.peerId) ?? this.stored.peers.find(p => p.id === input.peerId);
    if (!peer) return failure('NOT_FOUND', 'Tailscale device not found.');
    if (this.connections.size >= 16) return failure('VALIDATION', 'At most 16 remote hosts can be connected.');
    const next = { ...this.stored, peers: [...this.stored.peers.filter(p => p.id !== peer.id), { ...peer, online: false }] };
    try { await this.store.save(next); this.stored = next; } catch { return failure('STORAGE_FAILED', 'Remote device could not be remembered.'); }
    const connection: Connection = { clientId: randomUUID(), abort: new AbortController(), status: 'connecting', lastError: null, pending: 0 };
    this.connections.set(peer.id, connection); this.emitState(); void this.run(peer.id, connection); return this.getState();
  }
  private async run(peerId: string, connection: Connection): Promise<void> {
    let failures = 0;
    while (!connection.abort.signal.aborted && this.proxyUrl) {
      const cycle = new AbortController(); connection.cycle = cycle;
      const signal = AbortSignal.any([connection.abort.signal, cycle.signal]);
      try {
        requirePair(await this.send(peerId, '/v1/pair', { clientId: connection.clientId }, signal));
        signal.throwIfAborted();
        if (connection.abort.signal.aborted || this.connections.get(peerId) !== connection) return;
        connection.status = 'connected'; connection.lastError = null; failures = 0; this.emitState();
        while (!connection.abort.signal.aborted) {
          const raw = await this.send(peerId, '/v1/events', { clientId: connection.clientId }, signal);
          signal.throwIfAborted();
          const result = remotePollResponseSchema.parse(raw); if (!result.ok) throw new RemoteAccessError(result.error);
          if (connection.abort.signal.aborted || this.connections.get(peerId) !== connection) return;
          for (const event of result.value.events) this.emit({ ...event, peerId });
        }
      } catch (error) {
        if (connection.abort.signal.aborted) break;
        const detail = cycle.signal.aborted && connection.lastError?.code === 'APPROVAL_REQUIRED' ? connection.lastError : remoteFailure(error);
        this.accessFailure(peerId, detail, connection);
        if (detail.code === 'ACCESS_DENIED') break;
        const delay = detail.code === 'APPROVAL_REQUIRED' ? 1000 : Math.min(30_000, 1000 * 2 ** Math.min(failures++, 5));
        await waitForRemoteRetry(delay, signal);
      }
    }
  }
  private accessFailure(peerId: string, error: AppError, expected?: Connection, interrupt = false): void {
    const connection = this.connections.get(peerId); if (!connection || expected && connection !== expected) return;
    connection.lastError = error;
    connection.status = error.code === 'ACCESS_DENIED' ? 'rejected' : error.code === 'APPROVAL_REQUIRED' ? 'awaiting-approval' : 'reconnecting';
    // An approval response from another local view must interrupt a 20 second event poll.
    if (interrupt && error.code === 'APPROVAL_REQUIRED') connection.cycle?.abort();
    if (error.code === 'ACCESS_DENIED') { connection.abort.abort(); void Promise.allSettled([...this.views.values()].map(view => view.deny(peerId, error))); }
    this.emitState();
  }
  private send(peerId: string, requestPath: string, body: unknown, signal?: AbortSignal) {
    if (!this.proxyUrl) return Promise.reject(new Error('Remote access is stopped.'));
    return proxyRequest(this.proxyUrl, this.proxySecret, peerId, requestPath, body, signal);
  }
  private async disconnectNow(input: { peerId: string }) {
    await Promise.allSettled([...this.views.values()].map(view => view.disconnect(input.peerId)));
    const connection = this.connections.get(input.peerId);
    if (connection) {
      this.connections.delete(input.peerId); connection.abort.abort();
      if (this.proxyUrl) void this.send(input.peerId, '/v1/disconnect', { clientId: connection.clientId }).catch(() => {});
    }
    this.emitState(); return this.getState();
  }
  async request(raw: unknown): Promise<unknown> {
    const input = remoteForwardRequestSchema.safeParse(raw); if (!input.success) return failure('VALIDATION', 'Invalid remote operation.');
    const { peerId, method, payload } = input.data, connection = this.connections.get(peerId);
    if (connection?.status !== 'connected') return connection?.lastError ? { ok: false, error: connection.lastError } : failure('NATIVE_UNAVAILABLE', 'Remote host is not connected.', true);
    if (connection.pending >= 8) return failure('VALIDATION', 'Too many pending remote operations.', true);
    connection.pending++;
    try {
      const raw = await this.send(peerId, '/v1/request', { clientId: connection.clientId, method, payload }, connection.abort.signal);
      const result = responseSchemas[method].safeParse(raw);
      if (result.success && !result.data.ok && ['APPROVAL_REQUIRED', 'ACCESS_DENIED'].includes(result.data.error.code)) this.accessFailure(peerId, result.data.error, connection, true);
      return result.success ? result.data : failure('INTERNAL', 'Invalid remote host response.');
    } catch {
      return connection.lastError && ['APPROVAL_REQUIRED', 'ACCESS_DENIED'].includes(connection.lastError.code)
        ? { ok: false, error: connection.lastError }
        : failure('NATIVE_UNAVAILABLE', 'Remote request failed. Reconnect before retrying an uncertain mutation.', true);
    }
    finally { connection.pending--; }
  }
  private async stop(): Promise<void> {
    this.networkEpoch++; this.unsubscribeStatus?.(); this.unsubscribeStatus = undefined;
    await Promise.allSettled([...this.views.values()].map(view => view.disconnect()));
    if (this.poll) clearInterval(this.poll); this.poll = undefined;
    const connections = [...this.connections.entries()], proxyUrl = this.proxyUrl;
    for (const [, connection] of connections) connection.abort.abort(); this.connections.clear();
    this.proxyUrl = undefined; this.peers = [];
    if (proxyUrl && connections.length) {
      // Release host views and leases before closing our only route to them.
      const deadline = AbortSignal.timeout(1500);
      await Promise.allSettled(connections.map(([peerId, connection]) => proxyRequest(proxyUrl, this.proxySecret, peerId, '/v1/disconnect', { clientId: connection.clientId }, deadline)));
    }
    await this.helper.dispose(); await this.host?.dispose(); this.host = undefined;
    this.status = { status: 'disabled', loginUrl: null, error: null };
  }
  async dispose(): Promise<void> { this.disposed = true; await this.serialize(() => this.stop()); await Promise.allSettled([...this.views.values()].map(view => view.dispose())); this.listeners.clear(); }
}
