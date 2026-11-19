import { failure, type AppError, type TerminalApi, type TerminalEvent } from '../../../src/shared/contracts';
import type { RemoteHostEvent, RemoteMethod, RemoteRequestPayload, RemoteResponse } from '../../../src/shared/remote-contracts';
import { remotePairResponseSchema, remotePayloadSchemas, remotePeerIdSchema } from '../../../src/shared/remote-schemas';
import type { HostClient, ClientState, RemoteClientOptions } from '../client/host-client';
import { RemoteOperations } from '../client/operations';
import { connectionError, parseResponse, RemoteClientError, waitForRetry } from '../client/protocol';
import { z } from 'zod';
import { RustRemoteClient, rustRemoteMethods, type NativeRemoteBinding } from './remote-adapter';
import type { NativeTerminalBinding } from './adapter';
import { OperationPump, NativeOperationError } from './operation-pump';
import { coarsePollSchema, type NativeOperation, type CoarseHostEvent } from './native-events';
import { NativeTerminalDriver, registerNativeTerminal, type NativeExecute } from './native-terminal';

interface Run {
  abort: AbortController; pump?: OperationPump; suspended: boolean;
  restart: boolean; pairing: boolean; resume?: () => void; retry?: AbortController;
}
const allowedMethod = z.enum(rustRemoteMethods);

/** Existing mobile presentation, backed by the remote-only shared Rust client. */
export class RustHostClient extends RemoteOperations implements HostClient {
  private state: ClientState;
  private run?: Run;
  private closing: Promise<void> = Promise.resolve();
  private readonly listeners = new Set<(event: RemoteHostEvent) => void>();
  private readonly stateListeners = new Set<(state: ClientState) => void>();
  private readonly driver: NativeTerminalDriver;
  readonly terminalApi: Pick<TerminalApi, 'attachTerminal' | 'takeTerminalControl' | 'writeTerminal'
    | 'resizeTerminal' | 'acknowledgeTerminal' | 'detachTerminal'> = {
    attachTerminal: input => this.attachTerminal(input), takeTerminalControl: input => this.takeTerminalControl(input),
    writeTerminal: input => this.writeTerminal(input), resizeTerminal: input => this.resizeTerminal(input),
    acknowledgeTerminal: input => this.acknowledgeTerminal(input), detachTerminal: input => this.detachTerminal(input),
  };
  readonly subscribeTerminal = (listener: (event: TerminalEvent) => void): (() => void) =>
    this.subscribe(event => { if (event.type === 'terminal') listener(event.event); });

  constructor(readonly peerId: string, private readonly native: NativeRemoteBinding & NativeTerminalBinding,
    private readonly options: RemoteClientOptions,
  ) {
    super(); remotePeerIdSchema.parse(peerId);
    for (const delay of [options.approvalRetryMs ?? 1000, options.reconnectDelayMs ?? 1000,
      options.maxReconnectDelayMs ?? 30000]) {
      if (!Number.isFinite(delay) || delay < 1 || delay > 300000) throw new Error('Invalid remote retry delay.');
    }
    this.state = { peerId, status: 'disconnected', lastError: null, connectionId: null };
    this.driver = new NativeTerminalDriver(native, () => {
      const pump = this.run?.pump;
      if (!pump || this.state.status !== 'connected') throw new Error('Remote host is not connected.');
      const execute: NativeExecute = async (method, payload) =>
        (await pump.invoke(() => pump.remote.operation(method, payload))).result;
      return { remote: pump.remote, execute };
    });
    registerNativeTerminal(this.terminalApi, this.driver);
  }

  getState(): ClientState { return { ...this.state, lastError: this.state.lastError ? { ...this.state.lastError } : null }; }
  subscribe(listener: (event: RemoteHostEvent) => void): () => void {
    this.listeners.add(listener); return () => { this.listeners.delete(listener); };
  }
  subscribeState(listener: (state: ClientState) => void): () => void {
    this.stateListeners.add(listener); return () => { this.stateListeners.delete(listener); };
  }

  connect(): void {
    if (this.run) {
      if (!this.run.suspended) return;
      this.run.suspended = false; this.run.resume?.(); this.run.retry?.abort();
      return;
    }
    const run: Run = { abort: new AbortController(), suspended: false, restart: false, pairing: false }; this.run = run;
    this.change('connecting'); void this.loop(run);
  }
  reconnect(): void { this.stop(); this.connect(); }
  suspend(): void {
    const run = this.run; if (!run || run.suspended) return;
    run.suspended = true; run.retry?.abort();
    if (this.state.status !== 'connected') return;
    run.restart = true;
    this.change('reconnecting');
    // Retire terminal leases, not the device's pending pairing/approval attempt.
    this.closing = this.closing.then(() => this.driver.close());
    void this.closing.catch(cause => {
      if (this.active(run)) this.stopRun('reconnecting', connectionError(cause));
    });
  }
  stop(): void { this.stopRun('disconnected'); }

  async request<M extends RemoteMethod>(method: M, payload: RemoteRequestPayload<M>): Promise<RemoteResponse<M>> {
    // These operations must use the bound native engine, never a WebView/raw JS path.
    if (method === 'attachTerminal' || method === 'writeTerminal' || method === 'acknowledgeTerminal') {
      return parseResponse(method, failure('UNSUPPORTED', 'Use the native terminal binding for this operation.'));
    }
    const supported = allowedMethod.safeParse(method);
    if (!supported.success) return parseResponse(method, failure('UNSUPPORTED', 'This Rust client does not support this remote operation yet.'));
    const parsed = remotePayloadSchemas[supported.data].safeParse(payload);
    if (!parsed.success) return parseResponse(method, failure('VALIDATION', 'Invalid remote operation.'));
    const pump = this.run?.pump;
    if (!pump || this.state.status !== 'connected' || this.run?.suspended || this.run?.restart) {
      return parseResponse(method, this.state.lastError ? { ok: false, error: this.state.lastError }
        : failure('NATIVE_UNAVAILABLE', 'Remote host is not connected.', true));
    }
    try {
      const event = await pump.invoke(() => pump.remote.operation(supported.data, parsed.data));
      return parseResponse(method, event.result);
    } catch (error) {
      return parseResponse(method, error instanceof NativeOperationError && error.status === 1
        ? failure('VALIDATION', 'Too many pending remote operations.', true)
        : { ok: false, error: connectionError(error) });
    }
  }

  private active(run: Run): boolean { return this.run === run && !run.abort.signal.aborted; }
  private async resumed(run: Run): Promise<void> {
    while (this.active(run) && run.suspended) {
      await new Promise<void>(resolve => { run.resume = resolve; });
      run.resume = undefined;
    }
  }
  private async retry(run: Run, delay: number): Promise<void> {
    const retry = new AbortController(); run.retry = retry;
    const abort = () => retry.abort(); run.abort.signal.addEventListener('abort', abort, { once: true });
    try { await waitForRetry(delay, retry.signal); }
    catch (error) { if (!retry.signal.aborted) throw error; }
    finally {
      run.abort.signal.removeEventListener('abort', abort);
      if (run.retry === retry) run.retry = undefined;
    }
  }
  private change(status: ClientState['status'], error: AppError | null = null, connectionId: string | null = null): void {
    if (this.state.status === status && this.state.connectionId === connectionId
      && this.state.lastError?.code === error?.code && this.state.lastError?.message === error?.message) return;
    this.state = { peerId: this.peerId, status, lastError: error, connectionId };
    for (const listener of this.stateListeners) { try { listener(this.getState()); } catch { /* Isolate observers. */ } }
  }
  private observe(run: Run, event: NativeOperation): void {
    if (!this.active(run)) return;
    const connection = event.connection;
    if (connection.permission === 'rejected' || connection.status === 'rejected') {
      this.stopRun('rejected', { code: 'ACCESS_DENIED', message: 'Remote host access was denied.', retryable: false });
      return;
    }
    if (connection.status === 'connected' && (run.suspended || run.restart || run.pairing)) return;
    const status = connection.status === 'awaitingApproval' ? 'awaiting-approval'
      : connection.status === 'suspended' ? 'reconnecting' : connection.status;
    this.change(status, status === 'connected' ? null : this.state.lastError,
      status === 'connected' ? `${run.pump?.remote.handle}:${connection.connectionId}` : null);
    if (!run.suspended && !run.restart && event.internal && event.status === 0) {
      const poll = coarsePollSchema.safeParse(event.result);
      if (poll.success && poll.data.ok) this.deliver(run, poll.data.value.events);
    }
  }
  private deliver(run: Run, events: ReadonlyArray<CoarseHostEvent>): void {
    for (const item of events) {
      if (!this.active(run)) break;
      if (item.type === 'reattach') this.driver.recover(item.request);
      else this.emit(item);
    }
  }
  private emit(event: RemoteHostEvent): void {
    if (event.type === 'terminal') this.driver.event(event.event);
    for (const listener of this.listeners) { try { listener(event); } catch { /* Isolate observers. */ } }
  }

  private async loop(run: Run): Promise<void> {
    try {
      await this.closing;
      await this.resumed(run);
      if (!this.active(run)) return;
      const remote = await RustRemoteClient.create(this.native, this.peerId, this.options.createClientId());
      if (!this.active(run)) { await remote.close(); return; }
      const pump = new OperationPump(remote, event => this.observe(run, event), error => {
        if (this.active(run)) { this.stopRun('reconnecting'); this.change('reconnecting', connectionError(error)); }
      });
      run.pump = pump; pump.start();
      let paired = false, needsReconnect = false, failures = 0;
      while (this.active(run)) {
        try {
          await this.resumed(run);
          if (!this.active(run)) break;
          await this.closing;
          if (!this.active(run)) break;
          if (run.restart) { paired = false; needsReconnect = true; }
          if (!paired) {
            const reconnect = needsReconnect;
            needsReconnect = false; run.restart = false; run.pairing = true;
            const event = await pump.invoke(() => reconnect ? remote.reconnect() : remote.connect());
            const parsed = remotePairResponseSchema.safeParse(event.result);
            if (!parsed.success) throw new RemoteClientError({ code: 'UNSUPPORTED',
              message: 'This host does not support the required terminal client protocol.', retryable: false });
            const reply = parsed.data;
            if (!reply.ok) throw new RemoteClientError(reply.error);
            paired = true; failures = 0; run.pairing = false;
            await this.resumed(run);
            if (!this.active(run)) break;
            if (run.restart) continue;
            this.observe(run, event);
          }
          if (!this.active(run)) break;
          if (run.suspended || run.restart) continue;
          const event = await pump.invoke(() => remote.poll());
          const reply = coarsePollSchema.parse(event.result);
          if (!reply.ok) throw new RemoteClientError(reply.error);
          if (!this.active(run)) break;
          if (run.suspended || run.restart) continue;
          if (!event.internal) this.deliver(run, reply.value.events);
        } catch (cause) {
          if (!this.active(run)) break;
          run.pairing = false;
          if (cause instanceof NativeOperationError && cause.status === 1) {
            if (!run.suspended) await this.retry(run, 100);
            continue;
          }
          needsReconnect ||= paired;
          paired = false;
          const error = connectionError(cause);
          console.warn('Native V1 fixture', error.code, error.message);
          const denied = error.code === 'ACCESS_DENIED' || error.code === 'UNSUPPORTED' && !error.retryable;
          this.change(denied ? 'rejected' : error.code === 'APPROVAL_REQUIRED' ? 'awaiting-approval' : 'reconnecting', error);
          if (denied) { this.stopRun('rejected', error); break; }
          const delay = error.code === 'APPROVAL_REQUIRED' ? this.options.approvalRetryMs ?? 1000
            : Math.min(this.options.maxReconnectDelayMs ?? 30000, (this.options.reconnectDelayMs ?? 1000) * 2 ** Math.min(failures++, 5));
          if (!run.suspended) await this.retry(run, delay);
        }
      }
    } catch (cause) {
      if (this.active(run)) this.stopRun('disconnected', connectionError(cause));
    }
  }

  private stopRun(status: ClientState['status'], error: AppError | null = null): void {
    const run = this.run; this.run = undefined;
    run?.abort.abort(); run?.retry?.abort(); run?.resume?.();
    this.change(status, error);
    if (!run?.pump) return;
    const pump = run.pump;
    this.closing = this.closing.catch(() => undefined).then(async () => {
      try { await this.driver.close(); }
      finally { await pump.close(); }
    }).catch(() => undefined);
  }
}
