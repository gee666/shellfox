import { positiveDecimalSchema } from './identifiers';

interface Completion { operationId: string; status: number }
export interface CoarseBridge<E> {
  read(): Promise<E | null>;
  cancel(operationId: string): Promise<void>;
  close(): Promise<void>;
  /** Native signal that read() has results. Without it the pump pulls every
   * 100 ms; only the experimental v2 profiles still rely on that. */
  subscribe?(listener: () => void): () => void;
}

export class CoarseOperationError extends Error {
  constructor(readonly status: number) { super('Native remote operation failed.'); }
}
interface Pending<E> {
  resolve(event: E): void;
  reject(error: Error): void;
  timer: ReturnType<typeof setTimeout>;
}

/** Coarse bounded pull. Native owns all terminal chunks, rendering and ACKs. */
export class CoarseOperations<E extends Completion> {
  private readonly pending = new Map<string, Pending<E>>();
  private readonly early = new Map<string, E>();
  private starting = 0;
  private closed = false;
  private started = false;
  private draining = false;
  private signalled = false;
  private timer?: ReturnType<typeof setTimeout>;
  private unsubscribe?: () => void;

  constructor(private readonly bridge: CoarseBridge<E>,
    private readonly observe: (event: E) => void,
    private readonly failed: (error: Error) => void,
    private readonly limit = 8,
  ) {}

  start(): void {
    if (this.started || this.closed) return;
    this.started = true;
    this.unsubscribe = this.bridge.subscribe?.(() => this.wake());
    if (this.unsubscribe) this.wake(); else this.schedule();
  }

  async invoke(action: () => Promise<string>): Promise<E> {
    if (this.closed) throw new Error('Native client is closed.');
    if (this.pending.size + this.starting >= this.limit) throw new CoarseOperationError(1);
    this.starting++;
    let id: string;
    try { id = positiveDecimalSchema.parse(await action()); } finally { this.starting--; }
    if (this.closed) {
      void this.bridge.cancel(id).catch(() => undefined);
      throw new Error('Native client is closed.');
    }
    const early = this.early.get(id);
    this.early.delete(id);
    if (this.starting === 0) this.early.clear();
    if (early) return this.result(early);
    return new Promise<E>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        void this.bridge.cancel(id).catch(() => undefined);
        reject(new Error('Remote operation timed out. Its outcome may be uncertain.'));
      }, 35000);
      this.pending.set(id, { resolve, reject, timer });
    });
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    clearTimeout(this.timer);
    this.unsubscribe?.();
    for (const work of this.pending.values()) {
      clearTimeout(work.timer);
      work.reject(new Error('Remote client stopped.'));
    }
    this.pending.clear(); this.early.clear();
    await this.bridge.close();
  }

  private result(event: E): E {
    if (event.status !== 0) throw new CoarseOperationError(event.status);
    return event;
  }

  private schedule(): void {
    this.timer = setTimeout(() => { this.timer = undefined; void this.drain(); }, 100);
  }

  private wake(): void {
    this.signalled = true;
    if (!this.draining) void this.drain();
  }

  private async drain(): Promise<void> {
    if (this.closed) return;
    this.draining = true;
    this.signalled = false;
    let more = false;
    try {
      // Coarse operation/connection events only, never screen output.
      // Yield after eight events so one host cannot monopolize presentation.
      for (let count = 0; !this.closed; count++) {
        if (count === 8) { more = true; break; }
        const event = await this.bridge.read();
        if (this.closed || !event) break;
        this.observe(event);
        // Native polls are internally driven but may also return an ID to an
        // explicit JS poll call. Resolve that ID; never strand its waiter.
        const work = this.pending.get(event.operationId);
        if (work) {
          this.pending.delete(event.operationId); clearTimeout(work.timer);
          try { work.resolve(this.result(event)); } catch (error) {
            work.reject(error instanceof Error ? error : new Error('Invalid native result.'));
          }
        } else if (this.starting > 0) {
          if (this.early.size >= 64) throw new Error('Too many early native operation results.');
          this.early.set(event.operationId, event);
        }
      }
      this.draining = false;
      if (this.closed) return;
      if (!this.unsubscribe) this.schedule();
      else if (more || this.signalled) setTimeout(() => this.wake(), 0);
    } catch (cause) {
      this.draining = false;
      if (this.closed) return;
      const error = cause instanceof Error ? cause : new Error('Native client event failed.');
      this.failed(error);
      void this.close().catch(() => undefined);
    }
  }
}
