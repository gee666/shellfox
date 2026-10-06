import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { z } from 'zod';
import { failure, type Result } from '../../shared/contracts';
import { idSchema, nativeEventSchema, resultSchema } from '../../shared/schemas';
import type { NativeEvent } from '../../shared/native-port';
import { sessionWindowEventSchema, type SessionWindowEvent } from './session-windows';

export const MAX_FRAME = 1024 * 1024;
const envelope = z.discriminatedUnion('kind', [
  z.object({ version: z.literal(1), kind: z.literal('response'), id: idSchema, result: z.unknown() }).strict(),
  z.object({ version: z.literal(1), kind: z.literal('event'), event: nativeEventSchema }).strict(),
  z.object({ version: z.literal(1), kind: z.literal('session-window-event'), event: sessionWindowEventSchema }).strict(),
]);
type Pending = { schema: z.ZodType; finish: (result: Result<unknown>) => void; timer: ReturnType<typeof setTimeout> };

/** Private broker connection. No stderr, command lines or launch secrets are forwarded. */
export class BrokerTransport {
  private child: ChildProcessWithoutNullStreams | null = null;
  private buffer = Buffer.alloc(0);
  private readonly pending = new Map<string, Pending>();
  private closed = false;
  constructor(private readonly emit: (event: NativeEvent) => void, private readonly emitWindow: (event: SessionWindowEvent) => void = () => {}) {}

  start(executable: string): void {
    if (this.child || this.closed) throw new Error('Broker already started or disposed');
    const child = spawn(executable, ['broker'], { shell: false, windowsHide: true, stdio: 'pipe' });
    this.child = child;
    child.stdout.on('data', (data: Buffer) => this.accept(data));
    child.stderr.resume();
    child.on('error', () => this.fail('Native helper could not be started.'));
    child.on('exit', () => this.fail('Native helper exited. Terminals were not restarted or terminated.'));
    child.stdin.on('error', () => this.fail('Native helper input pipe closed.'));
  }

  get processId(): number | null { return this.closed ? null : this.child?.pid ?? null; }

  // Exposed only to co-located protocol tests, not renderer IPC.
  accept(data: Buffer): void {
    if (this.closed) return;
    this.buffer = Buffer.concat([this.buffer, data]);
    for (;;) {
      const newline = this.buffer.indexOf(10);
      if (newline < 0) {
        if (this.buffer.length > MAX_FRAME) this.fail('Native frame exceeds 1 MiB.');
        return;
      }
      if (newline > MAX_FRAME) { this.fail('Native frame exceeds 1 MiB.'); return; }
      const frame = this.buffer.subarray(0, newline);
      this.buffer = this.buffer.subarray(newline + 1);
      try {
        const message = envelope.parse(JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(frame)));
        if (message.kind === 'event') this.emit(message.event);
        else if (message.kind === 'session-window-event') this.emitWindow(message.event);
        else {
          const request = this.pending.get(message.id);
          if (!request) { this.fail('Native response does not match an outstanding request.'); return; }
          const result = resultSchema(request.schema).parse(message.result) as Result<unknown>;
          this.pending.delete(message.id); clearTimeout(request.timer); request.finish(result);
        }
      } catch { this.fail('Native helper returned an invalid protocol frame.'); return; }
    }
  }

  async request<T>(method: string, payload: unknown, schema: z.ZodType<T>): Promise<Result<T>> {
    if (this.closed || !this.child) return failure('NATIVE_UNAVAILABLE', 'Native helper is not connected.');
    const id = randomUUID();
    const data = Buffer.from(JSON.stringify({ version: 1, id, kind: 'request', method, payload }) + '\n');
    if (data.length > MAX_FRAME) return failure('VALIDATION', 'Native request exceeds 1 MiB.');
    if (this.pending.size >= 1000) return failure('NATIVE_UNAVAILABLE', 'Too many outstanding native requests.');
    return new Promise<Result<T>>(resolve => {
      const timer = setTimeout(() => {
        // Launch/control may have happened. Disconnect rather than accepting an uncorrelated late response.
        this.fail('Native request timed out. A terminal may already exist; no automatic retry was made.');
      }, 10_000);
      this.pending.set(id, { schema, finish: result => resolve(result as Result<T>), timer });
      this.child!.stdin.write(data, error => { if (error) this.fail('Native request could not be delivered.'); });
    });
  }

  private fail(message: string): void {
    if (this.closed) return;
    this.closed = true;
    this.buffer = Buffer.alloc(0);
    for (const request of this.pending.values()) { clearTimeout(request.timer); request.finish(failure('NATIVE_UNAVAILABLE', message)); }
    this.pending.clear();
    this.child?.stdin.end();
    // Only our private broker is stopped. It does not own the shell lifetimes.
    this.child?.kill();
    this.emit({ type: 'unavailable', error: { code: 'NATIVE_UNAVAILABLE', message, retryable: false } });
  }

  async dispose(): Promise<void> {
    this.closed = true;
    for (const request of this.pending.values()) { clearTimeout(request.timer); request.finish(failure('NATIVE_UNAVAILABLE', 'Native backend disposed.')); }
    this.pending.clear();
    const child = this.child;
    if (!child) return;
    child.stdin.end();
    if (child.exitCode !== null) return;
    await new Promise<void>(resolve => {
      const timer = setTimeout(() => { child.kill(); resolve(); }, 2000);
      child.once('exit', () => { clearTimeout(timer); resolve(); });
    });
  }
}
