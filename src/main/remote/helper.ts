import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { z } from 'zod';
import { remoteStatusSchema } from '../../shared/remote-schemas';
export type HelperStatus = z.infer<typeof remoteStatusSchema>;
const statusEventSchema = z.object({ event: z.literal('status'), payload: remoteStatusSchema }).strict();
const replySchema = z.discriminatedUnion('ok', [z.object({ id: z.string(), ok: z.literal(true), value: z.unknown() }), z.object({ id: z.string(), ok: z.literal(false), error: z.unknown() })]);
export interface HelperPort { command(method: string, payload: unknown): Promise<unknown>; dispose(): Promise<void>; subscribeStatus?(listener: (status: HelperStatus) => void): () => void }
export class RemoteHelper implements HelperPort {
  private child?: ChildProcessWithoutNullStreams;
  private nextId = 0;
  private failed = false;
  private readonly statuses = new Set<(status: HelperStatus) => void>();
  subscribeStatus(listener: (status: HelperStatus) => void): () => void { this.statuses.add(listener); return () => { this.statuses.delete(listener); }; }
  private emitStatus(status: HelperStatus): void { for (const listener of this.statuses) { try { listener({ ...status, loginUrl: status.status === 'login-required' ? status.loginUrl : null }); } catch { /* Consumers cannot interrupt native replies. */ } } }
  private readonly pending = new Map<string, { resolve: (value: unknown) => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> }>();
  constructor(private readonly executable: string) {}
  private start(): ChildProcessWithoutNullStreams {
    if (this.child) return this.child;
    const child = spawn(this.executable, [], { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
    this.child = child; let buffered = '';
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
    child.once('error', () => { if (this.child === child) this.fail(); }); child.once('exit', () => { if (this.child === child) this.fail(); });
    return child;
  }
  private fail(): void {
    this.child = undefined; this.failed = true;
    for (const entry of this.pending.values()) { clearTimeout(entry.timer); entry.reject(new Error('Tailscale helper stopped.')); }
    this.pending.clear();
    this.emitStatus({ status: 'error', loginUrl: null, error: 'Tailscale helper stopped.' });
  }
  command(method: string, payload: unknown): Promise<unknown> {
    if (this.failed && method !== 'start') return Promise.reject(new Error('Tailscale helper stopped. Disable and re-enable remote access.'));
    if (method === 'start') this.failed = false;
    if (this.pending.size >= 32) return Promise.reject(new Error('Too many Tailscale commands.'));
    return new Promise((resolve, reject) => {
      const id = String(++this.nextId);
      const timer = setTimeout(() => { this.pending.delete(id); reject(new Error('Tailscale helper timed out.')); }, 30_000);
      this.pending.set(id, { resolve, reject, timer });
      try {
        const child = this.start();
        child.stdin.write(JSON.stringify({ id, method, payload }) + '\n', error => { if (error && this.child === child) this.fail(); });
      }
      catch { this.fail(); }
    });
  }
  async dispose(): Promise<void> {
    const child = this.child; if (!child) return;
    try { await this.command('stop', {}); } catch { /* Shutdown still terminates the owned helper. */ }
    child.kill(); this.fail();
  }
}
