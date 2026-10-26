import { failure, type AppError, type TerminalApi, type TerminalEvent } from '../../../src/shared/contracts';
import type { RemoteHostEvent, RemoteMethod, RemotePeer, RemoteRequestPayload, RemoteResponse } from '../../../src/shared/remote-contracts';
import { remoteForwardRequestSchema, remotePairResponseSchema, remotePeerIdSchema, remotePollResponseSchema, remotePayloadSchemas } from '../../../src/shared/remote-schemas';
import { idSchema, requestSchemas, responseSchemas } from '../../../src/shared/schemas';
import { RemoteOperations } from './operations';
import { connectionError, encodeBody, parseJson, parseResponse, RemoteClientError, untilStopped, waitForRetry, type NativeRemoteTransport, type RemotePath } from './protocol';

export interface ClientState {
  readonly peerId: string;
  readonly status: RemotePeer['connection'];
  readonly lastError: AppError | null;
  /** Changes on every successful pairing. All terminal views must be reattached. */
  readonly connectionId: string | null;
}
export interface RemoteClientOptions {
  /** Supply cryptographically random UUIDs from the native platform. Never persist these. */
  createClientId(): string;
  approvalRetryMs?: number;
  reconnectDelayMs?: number;
  maxReconnectDelayMs?: number;
}
interface Cycle { clientId: string; abort: AbortController; pending: number; failure?: AppError }
interface Run { clientId: string; abort: AbortController; cycle?: Cycle }

/** One authenticated host view and event poll per peer, independent of React/UI lifetime. */
export class RemoteClient extends RemoteOperations {
  private state: ClientState;
  private run?: Run;
  private readonly listeners = new Set<(event: RemoteHostEvent) => void>();
  private readonly stateListeners = new Set<(state: ClientState) => void>();
  private readonly attachments = new Map<string, string>();
  private epoch = 0;
  readonly terminalApi: Pick<TerminalApi, 'attachTerminal' | 'writeTerminal' | 'resizeTerminal' | 'acknowledgeTerminal' | 'detachTerminal'> = {
    attachTerminal: input => this.attachTerminal(input),
    writeTerminal: input => this.writeTerminal(input),
    resizeTerminal: input => this.resizeTerminal(input),
    acknowledgeTerminal: input => this.acknowledgeTerminal(input),
    detachTerminal: input => this.detachTerminal(input),
  };
  readonly subscribeTerminal = (listener: (event: TerminalEvent) => void): (() => void) =>
    this.subscribe(event => { if (event.type === 'terminal') listener(event.event); });
  private readonly approvalRetryMs: number;
  private readonly reconnectDelayMs: number;
  private readonly maxReconnectDelayMs: number;

  constructor(readonly peerId: string, private readonly transport: NativeRemoteTransport, private readonly options: RemoteClientOptions) {
    super(); remotePeerIdSchema.parse(peerId);
    this.approvalRetryMs = options.approvalRetryMs ?? 1000;
    this.reconnectDelayMs = options.reconnectDelayMs ?? 1000;
    this.maxReconnectDelayMs = options.maxReconnectDelayMs ?? 30_000;
    for (const delay of [this.approvalRetryMs, this.reconnectDelayMs, this.maxReconnectDelayMs]) {
      if (!Number.isFinite(delay) || delay < 1 || delay > 300_000) throw new Error('Invalid remote retry delay.');
    }
    this.state = { peerId, status: 'disconnected', lastError: null, connectionId: null };
  }

  getState(): ClientState { return { ...this.state, lastError: this.state.lastError ? { ...this.state.lastError } : null }; }
  subscribe(listener: (event: RemoteHostEvent) => void): () => void {
    this.listeners.add(listener); return () => { this.listeners.delete(listener); };
  }
  subscribeState(listener: (state: ClientState) => void): () => void {
    this.stateListeners.add(listener); return () => { this.stateListeners.delete(listener); };
  }
  private emit(event: RemoteHostEvent): void {
    for (const listener of this.listeners) { try { listener(event); } catch { /* Isolate observers. */ } }
  }
  private setState(status: ClientState['status'], lastError: AppError | null = null, connectionId: string | null = null): void {
    this.state = { peerId: this.peerId, status, lastError, connectionId };
    for (const listener of this.stateListeners) { try { listener(this.getState()); } catch { /* Isolate observers. */ } }
  }

  /** Starts pairing in the background. Pending approval retries; denial needs explicit connect(). */
  connect(): void {
    if (this.run) return;
    const run: Run = { clientId: idSchema.parse(this.options.createClientId()), abort: new AbortController() }; this.run = run;
    this.setState('connecting');
    void this.loop(run);
  }
  reconnect(): void { this.stop(); this.connect(); }
  /** Stops retries/polling immediately. Host cleanup is best effort and never closes shells. */
  stop(): void {
    const run = this.run; this.run = undefined;
    run?.abort.abort(); run?.cycle?.abort.abort();
    if (run) this.release(run.clientId);
    this.invalidate({ code: 'NATIVE_UNAVAILABLE', message: 'Remote view stopped. Reattach after connecting.', retryable: true });
    if (!this.run) this.setState('disconnected');
  }
  private active(run: Run, cycle?: Cycle): boolean {
    return this.run === run && !run.abort.signal.aborted && (!cycle || run.cycle === cycle && !cycle.abort.signal.aborted);
  }
  private release(clientId: string): void {
    try { void this.transport.request(this.peerId, '/v1/disconnect', encodeBody({ clientId })).catch(() => {}); }
    catch { /* Native transport may already be stopped. */ }
  }
  private async send(cycle: Cycle, path: RemotePath, body: unknown): Promise<unknown> {
    const work = this.transport.request(this.peerId, path, encodeBody(body)).then(json => {
      // A cancelled native pairing can finish after stop/disconnect. Withdraw that late view too.
      if (path === '/v1/pair' && this.run?.clientId !== cycle.clientId) this.release(cycle.clientId);
      return parseJson(json);
    });
    return untilStopped(work, cycle.abort.signal);
  }
  private invalidate(error: AppError): void {
    const attached = [...this.attachments]; this.attachments.clear();
    for (const [tabId, generation] of attached) this.emit({ type: 'terminal', event: { type: 'error', tabId, generation, error } });
  }
  private interrupt(cycle: Cycle, error: AppError): void {
    const run = this.run;
    cycle.failure = error; cycle.abort.abort();
    if (this.run === run) this.setState(error.code === 'ACCESS_DENIED' ? 'rejected' : error.code === 'APPROVAL_REQUIRED' ? 'awaiting-approval' : 'reconnecting', error);
    if (this.run === run) this.invalidate(error);
  }
  private async loop(run: Run): Promise<void> {
    let failures = 0;
    while (this.active(run)) {
      let cycle: Cycle | undefined;
      try {
        cycle = { clientId: run.clientId, abort: new AbortController(), pending: 0 };
        run.cycle = cycle;
        const pair = remotePairResponseSchema.parse(await this.send(cycle, '/v1/pair', { clientId: cycle.clientId }));
        if (!pair.ok) throw new RemoteClientError(pair.error);
        if (!this.active(run, cycle)) return;
        failures = 0; this.setState('connected', null, `${cycle.clientId}:${++this.epoch}`);
        while (this.active(run, cycle)) {
          const result = remotePollResponseSchema.parse(await this.send(cycle, '/v1/events', { clientId: cycle.clientId }));
          if (!result.ok) throw new RemoteClientError(result.error);
          if (!this.active(run, cycle)) return;
          for (const event of result.value.events) {
            if (!this.active(run, cycle)) return;
            if (event.type === 'terminal' && event.event.type === 'exit') this.attachments.delete(event.event.tabId);
            this.emit(event);
          }
        }
        if (cycle.failure) throw new RemoteClientError(cycle.failure);
      } catch (error) {
        if (!this.active(run)) return;
        const detail = cycle?.failure ?? connectionError(error);
        const denied = detail.code === 'ACCESS_DENIED';
        if (this.state.lastError !== detail) this.setState(denied ? 'rejected' : detail.code === 'APPROVAL_REQUIRED' ? 'awaiting-approval' : 'reconnecting', detail);
        if (!this.active(run)) return;
        this.invalidate(detail);
        if (!this.active(run)) return;
        if (denied) this.run = undefined;
        if (denied || !cycle) return;
        cycle.abort.abort();
        if (run.cycle === cycle) run.cycle = undefined;
        const delay = detail.code === 'APPROVAL_REQUIRED' ? this.approvalRetryMs : Math.min(this.maxReconnectDelayMs, this.reconnectDelayMs * 2 ** Math.min(failures++, 10));
        await waitForRetry(delay, run.abort.signal);
      } finally {
        cycle?.abort.abort();
        if (!this.active(run)) this.release(run.clientId);
      }
    }
  }

  async request<M extends RemoteMethod>(method: M, payload: RemoteRequestPayload<M>): Promise<RemoteResponse<M>> {
    const invalid = () => parseResponse(method, failure('VALIDATION', 'Invalid remote operation.'));
    let validated: unknown;
    try {
      // Unlike the local schema, remote env names use the host's case-sensitivity policy.
      const envelope = remoteForwardRequestSchema.safeParse({ peerId: this.peerId, method, payload });
      if (!envelope.success) return invalid();
      validated = remotePayloadSchemas[method].parse(payload);
    } catch { return invalid(); }
    const run = this.run, cycle = run?.cycle;
    if (!run || !cycle || !this.active(run, cycle) || this.state.status !== 'connected') {
      return parseResponse(method, this.state.lastError ? { ok: false, error: this.state.lastError } : failure('NATIVE_UNAVAILABLE', 'Remote host is not connected.', true));
    }
    if (cycle.pending >= 8) return parseResponse(method, failure('VALIDATION', 'Too many pending remote operations.', true));
    cycle.pending++;
    try {
      const raw = await this.send(cycle, '/v1/request', { clientId: cycle.clientId, method, payload: validated });
      if (!this.active(run, cycle)) throw new Error('Remote view changed.');
      const result = parseResponse(method, raw);
      if (!result.ok && ['AUTH_FAILED', 'APPROVAL_REQUIRED', 'ACCESS_DENIED'].includes(result.error.code)) this.interrupt(cycle, result.error);
      if (result.ok && method === 'attachTerminal') {
        const attachment = responseSchemas.attachTerminal.parse(result);
        if (attachment.ok) this.attachments.set(attachment.value.tabId, attachment.value.generation);
      }
      if (result.ok && method === 'detachTerminal') this.attachments.delete(requestSchemas.detachTerminal.parse(validated).tabId);
      return result;
    } catch (error) {
      const detail = cycle.failure ?? connectionError(error);
      // Local encoding failures never reached the host and do not invalidate its view.
      if (this.active(run, cycle) && detail.code !== 'VALIDATION') this.interrupt(cycle, detail);
      return parseResponse(method, { ok: false, error: detail });
    } finally { cycle.pending--; }
  }
}
