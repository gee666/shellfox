import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { z } from 'zod';
import type { HelperLaunch } from '../../host/stable-helper';
import { remoteStatusSchema } from '../../shared/remote-schemas';
export type HelperStatus = z.infer<typeof remoteStatusSchema>;
const statusEventSchema = z.object({ event: z.literal('status'), payload: remoteStatusSchema }).strict();
const replySchema = z.discriminatedUnion('ok', [z.object({ id: z.string(), ok: z.literal(true), value: z.unknown() }), z.object({ id: z.string(), ok: z.literal(false), error: z.unknown() })]);
export interface HelperPort { command(method: string, payload: unknown): Promise<unknown>; dispose(): Promise<void>; subscribeStatus?(listener: (status: HelperStatus) => void): () => void }

async function bounded<T>(operation: Promise<T>, milliseconds: number): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([operation, new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject(new Error('Tailscale helper close is unconfirmed. Retry shutdown before restarting or updating it.')), milliseconds);
    })]);
  } finally { clearTimeout(timer); }
}

export class RemoteHelper implements HelperPort {
  private child?: ChildProcessWithoutNullStreams;
  private closed?: Promise<void>;
  private starting?: Promise<ChildProcessWithoutNullStreams>;
  private stopping = false;
  private disposing?: Promise<void>;
  private nextId = 0;
  private failed = false;
  private readonly statuses = new Set<(status: HelperStatus) => void>();
  subscribeStatus(listener: (status: HelperStatus) => void): () => void { this.statuses.add(listener); return () => { this.statuses.delete(listener); }; }
  private emitStatus(status: HelperStatus): void { for (const listener of this.statuses) { try { listener({ ...status, loginUrl: status.status === 'login-required' ? status.loginUrl : null }); } catch { /* Consumers cannot interrupt native replies. */ } } }
  private readonly pending = new Map<string, { resolve: (value: unknown) => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> }>();
  constructor(private readonly executable: string | (() => Promise<HelperLaunch>)) {}
  private start(): ChildProcessWithoutNullStreams | Promise<ChildProcessWithoutNullStreams> {
    if (this.child) return this.child;
    if (typeof this.executable === 'string') return this.launch({ executable: this.executable, args: [] });
    return this.starting ??= this.executable().then(launch => {
      if (this.stopping) throw new Error('Tailscale helper is stopping.');
      if (!this.pending.size) throw new Error('Tailscale helper startup no longer has a pending command.');
      return this.launch(launch);
    }).finally(() => { this.starting = undefined; });
  }
  private launch(launch: HelperLaunch): ChildProcessWithoutNullStreams {
    const child = spawn(launch.executable, launch.args, { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
    this.child = child; this.failed = false; let buffered = '';
    this.closed = new Promise(resolve => child.once('close', () => {
      if (this.child === child) { this.fail(); this.child = undefined; }
      resolve();
    }));
    child.stdin.on('error', () => { if (this.child === child) { this.fail(); child.kill(); } });
    child.stdout.setEncoding('utf8');
    child.stdout.on('data', (chunk: string) => {
      if (this.child !== child) return;
      buffered += chunk;
      if (buffered.length > 1024 * 1024) { this.fail(); child.kill(); return; }
      let newline: number;
      while ((newline = buffered.indexOf('\n')) >= 0) {
        const line = buffered.slice(0, newline); buffered = buffered.slice(newline + 1);
        try {
          const raw: unknown = JSON.parse(line);
          const event = statusEventSchema.safeParse(raw); if (event.success) { this.emitStatus(event.data.payload); continue; }
          const parsed = replySchema.safeParse(raw); if (!parsed.success) continue;
          const entry = this.pending.get(parsed.data.id); if (!entry) continue;
          this.pending.delete(parsed.data.id); clearTimeout(entry.timer);
          if (parsed.data.ok) entry.resolve(parsed.data.value); else entry.reject(new Error('Tailscale helper command failed.'));
        } catch { /* Ignore malformed lines. Pending commands still time out. */ }
      }
    });
    // Never forward helper diagnostics or auth material into renderer/logs.
    child.stderr.resume();
    child.once('error', () => { if (this.child === child) this.fail(); });
    child.once('exit', () => { if (this.child === child) this.fail(); });
    return child;
  }
  private fail(): void {
    // Only close releases the original ChildProcess. Error, exit and kill are not close receipts.
    this.failed = true;
    for (const entry of this.pending.values()) { clearTimeout(entry.timer); entry.reject(new Error('Tailscale helper stopped.')); }
    this.pending.clear();
    this.emitStatus({ status: 'error', loginUrl: null, error: 'Tailscale helper stopped.' });
  }
  command(method: string, payload: unknown): Promise<unknown> {
    if (this.stopping) return Promise.reject(new Error('Tailscale helper is stopping. Retry shutdown before restarting.'));
    if (this.failed && (method !== 'start' || this.child)) return Promise.reject(new Error('Tailscale helper stopped. Disable and re-enable remote access after its original process closes.'));
    return this.request(method, payload);
  }
  private request(method: string, payload: unknown): Promise<unknown> {
    if (this.pending.size >= 32) return Promise.reject(new Error('Too many Tailscale commands.'));
    return new Promise((resolve, reject) => {
      const id = String(++this.nextId);
      const timer = setTimeout(() => { this.pending.delete(id); reject(new Error('Tailscale helper timed out.')); }, 30_000);
      this.pending.set(id, { resolve, reject, timer });
      const write = (child: ChildProcessWithoutNullStreams) => {
        if (!this.pending.has(id)) return;
        child.stdin.write(JSON.stringify({ id, method, payload }) + '\n', error => { if (error && this.child === child) { this.fail(); child.kill(); } });
      };
      const failed = (cause: unknown) => {
        this.pending.delete(id); clearTimeout(timer);
        reject(cause instanceof Error ? cause : new Error('Tailscale helper could not start.'));
        if (this.child) { this.fail(); this.child.kill(); } else this.failed = true;
      };
      try {
        const child = this.start();
        if (child instanceof Promise) void child.then(write).catch(failed); else write(child);
      } catch (error) { failed(error); }
    });
  }
  dispose(): Promise<void> {
    return this.disposing ??= this.shutdown().finally(() => { this.disposing = undefined; });
  }
  private async shutdown(): Promise<void> {
    this.stopping = true;
    // A pending verified publication must finish before we can confirm that no spawn is possible.
    if (this.starting) await bounded(this.starting.catch(() => {}), 5000);
    const child = this.child;
    if (child) {
      if (!this.failed) {
        try { await bounded(this.request('stop', {}), 2000); } catch { /* Terminate the same retained child below. */ }
      }
      if (this.child === child) { child.kill(); this.fail(); }
      await bounded(this.closed ?? Promise.reject(new Error('Missing helper close receipt.')), 5000);
    }
    this.stopping = false;
  }
}
