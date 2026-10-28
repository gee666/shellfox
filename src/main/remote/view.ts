import { randomUUID } from 'node:crypto';
import { failure, type AppError } from '../../shared/contracts';
import { terminalResponseCompatibility } from '../../shared/terminal-compat';
import { uploadResponseCompatibility } from '../../shared/terminal-upload-schemas';
import type { RemoteEvent, RemotePeer } from '../../shared/remote-contracts';
import { remoteForwardRequestSchema, remotePollResponseSchema, responseSchemas, requestSchemas } from '../../shared/remote-schemas';

export interface RemoteViewPort {
  request(raw: unknown): Promise<unknown>;
  subscribe(listener: (event: RemoteEvent) => void): () => void;
  disconnect(peerId?: string): Promise<void>;
  deny(peerId: string, error: AppError): Promise<void>;
  dispose(): Promise<void>;
}
interface Connection { clientId: string; abort: AbortController; ready: Promise<void>; pending: number; pairing: boolean; paired: boolean; failure?: AppError }
type Send = (peerId: string, path: string, body: unknown, signal?: AbortSignal) => Promise<unknown>;
import { RemoteAccessError, requirePair, remoteFailure } from './pairing';
/** One independent upstream terminal view for each authenticated local client. */
export class RemoteView implements RemoteViewPort {
  private readonly connections = new Map<string, Connection>();
  private readonly listeners = new Set<(event: RemoteEvent) => void>();
  private readonly attachments = new Map<string, Map<string, { generation: string; controlEpoch: number }>>();
  private readonly cleanup = new Set<Promise<unknown>>();
  private disposal?: Promise<void>;
  private emit(event: RemoteEvent): void { for (const listener of this.listeners) { try { listener(event); } catch { /* One observer cannot block the view. */ } } }
  private disposed = false;
  constructor(private readonly state: (peerId: string) => { status: RemotePeer['connection']; lastError?: AppError | null } | undefined, private readonly send: Send, private readonly removed: () => void, private readonly accessFailure: (peerId: string, error: AppError) => void = () => {}) {}
  subscribe(listener: (event: RemoteEvent) => void): () => void { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; }
  private connection(peerId: string): Connection {
    const existing = this.connections.get(peerId);
    if (existing) {
      if (!existing.paired && !existing.pairing && !['ACCESS_DENIED', 'UNSUPPORTED'].includes(existing.failure?.code ?? '')) existing.ready = this.pair(peerId, existing);
      return existing;
    }
    if (this.connections.size >= 16) throw new Error('Too many remote host views.');
    const connection: Connection = { clientId: randomUUID(), abort: new AbortController(), ready: Promise.resolve(), pending: 0, pairing: false, paired: false };
    this.connections.set(peerId, connection);
    connection.ready = this.pair(peerId, connection); return connection;
  }
  private async pair(peerId: string, connection: Connection): Promise<void> {
    connection.pairing = true;
    try {
      requirePair(await this.send(peerId, '/v1/pair', { clientId: connection.clientId }, connection.abort.signal));
      if (connection.abort.signal.aborted) throw new Error('Remote view closed.');
      connection.paired = true; connection.failure = undefined; void this.poll(peerId, connection);
    } catch (error) {
      connection.failure = remoteFailure(error);
      if (!connection.abort.signal.aborted && this.connections.get(peerId) === connection && ['APPROVAL_REQUIRED', 'ACCESS_DENIED'].includes(connection.failure.code)) this.accessFailure(peerId, connection.failure);
      throw error;
    } finally { connection.pairing = false; }
  }
  private async poll(peerId: string, connection: Connection): Promise<void> {
    try {
      while (!connection.abort.signal.aborted) {
        const result = remotePollResponseSchema.parse(await this.send(peerId, '/v1/events', { clientId: connection.clientId }, connection.abort.signal));
        if (!result.ok) throw new RemoteAccessError(result.error);
        if (connection.abort.signal.aborted || this.connections.get(peerId) !== connection) return;
        for (const event of result.value.events) {
          if (event.type === 'terminal' && event.event.type === 'control') this.attachments.get(peerId)?.set(event.event.tabId, { generation: event.event.generation, controlEpoch: event.event.controlEpoch });
          if (event.type === 'terminal' && event.event.type === 'exit') this.attachments.get(peerId)?.delete(event.event.tabId);
          this.emit({ ...event, peerId });
        }
      }
    } catch (error) {
      const detail = remoteFailure(error);
      if (!connection.abort.signal.aborted && this.connections.get(peerId) === connection && ['APPROVAL_REQUIRED', 'ACCESS_DENIED'].includes(detail.code)) this.accessFailure(peerId, detail);
      // Reconnect on the next explicit request. The caller must reattach and restore output.
      if (this.connections.get(peerId) === connection) {
        for (const [tabId, identity] of this.attachments.get(peerId) ?? []) this.emit({ type: 'terminal', peerId, event: { type: 'error', tabId, ...identity, error: detail } });
        await this.disconnect(peerId);
      }
    }
  }
  async request(raw: unknown): Promise<unknown> {
    const parsed = remoteForwardRequestSchema.safeParse(raw);
    if (!parsed.success) return failure('VALIDATION', 'Invalid remote operation.');
    const input = parsed.data;
    const state = this.state(input.peerId);
    if (this.disposed || state?.status !== 'connected') return state?.lastError ? { ok: false, error: state.lastError } : failure('NATIVE_UNAVAILABLE', 'Remote host is not connected.', true);
    try {
      const connection = this.connection(input.peerId);
      if (connection.pending >= 8) return failure('VALIDATION', 'Too many pending remote view operations.', true);
      connection.pending++;
      try {
        await connection.ready;
        const result = await this.send(input.peerId, '/v1/request', { clientId: connection.clientId, method: input.method, payload: input.payload }, connection.abort.signal);
        const validated = responseSchemas[input.method].parse(uploadResponseCompatibility(input.method, terminalResponseCompatibility(input.method, result)));
        if (!connection.abort.signal.aborted && this.connections.get(input.peerId) === connection && !validated.ok && ['APPROVAL_REQUIRED', 'ACCESS_DENIED'].includes(validated.error.code)) this.accessFailure(input.peerId, validated.error);
        if (validated.ok && input.method === 'attachTerminal') {
          const attached = responseSchemas.attachTerminal.parse(validated);
          if (attached.ok) { const tabs = this.attachments.get(input.peerId) ?? new Map<string, { generation: string; controlEpoch: number }>(); tabs.set(attached.value.tabId, { generation: attached.value.generation, controlEpoch: attached.value.controlEpoch }); this.attachments.set(input.peerId, tabs); }
        }
        if (validated.ok && input.method === 'detachTerminal') {
          const identity = requestSchemas.detachTerminal.parse(input.payload);
          this.attachments.get(input.peerId)?.delete(identity.tabId);
        }
        return validated;
      } finally { connection.pending--; }
    } catch (error) { return { ok: false, error: error instanceof RemoteAccessError ? error.detail : state?.lastError ?? remoteFailure(error) }; }
  }
  async deny(peerId: string, error: AppError): Promise<void> {
    for (const [tabId, identity] of this.attachments.get(peerId) ?? []) this.emit({ type: 'terminal', peerId, event: { type: 'error', tabId, ...identity, error } });
    await this.disconnect(peerId);
  }
  async disconnect(peerId?: string): Promise<void> {
    const selected = [...this.connections].filter(([id]) => peerId === undefined || id === peerId);
    for (const [id, connection] of selected) { this.connections.delete(id); this.attachments.delete(id); connection.abort.abort(); }
    for (const [id, connection] of selected) {
      const task = this.send(id, '/v1/disconnect', { clientId: connection.clientId }, AbortSignal.timeout(1500)).catch(() => {});
      this.cleanup.add(task); void task.finally(() => this.cleanup.delete(task));
    }
    await Promise.allSettled(this.cleanup);
  }
  dispose(): Promise<void> {
    return this.disposal ??= (async () => { this.disposed = true; await this.disconnect(); this.listeners.clear(); this.removed(); })();
  }
}
