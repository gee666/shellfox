import { randomUUID } from 'node:crypto';
import type { HostEvent } from '../shared/host-contracts';
import type { TerminalControlView } from '../main/terminal/control';
import type { HostSessionService } from './operations';
import type { RemoteViewPort } from '../main/remote/view';

export interface RpcClient {
  id: string; delivery: TerminalControlView; events: HostEvent[]; bytes: number; touched: number;
  busy: number; polling: boolean; notify?: () => void; remote?: RemoteViewPort; unsubscribeRemote?: () => void;
}
export class RpcClients {
  private readonly clients = new Map<string, RpcClient>();
  private readonly timer: ReturnType<typeof setInterval>;
  private readonly cleanup = new Set<Promise<void>>();
  constructor(private readonly service: HostSessionService, private readonly remoteView?: (id: string) => RemoteViewPort, private readonly onDrop?: (id: string) => void) {
    this.timer = setInterval(() => { for (const client of this.clients.values()) if (Date.now() - client.touched > 60_000 && !client.busy) this.drop(client.id); }, 15_000);
    this.timer.unref();
  }
  open(): RpcClient {
    if (this.clients.size >= 64) throw new Error('Too many host clients.');
    const id = randomUUID();
    const client: RpcClient = { id, delivery: this.service.createTerminalView(event => this.push(client, { type: 'terminal', event }), tabId => this.purge(client, tabId)), events: [], bytes: 0, touched: Date.now(), busy: 0, polling: false };
    client.remote = this.remoteView?.(id);
    client.unsubscribeRemote = client.remote?.subscribe(event => this.push(client, { type: 'remote', event }));
    this.clients.set(id, client); return client;
  }
  get(id: string | undefined): RpcClient {
    const client = id ? this.clients.get(id) : undefined;
    if (!client) throw new Error('Host client expired. Reconnect to the host.');
    client.touched = Date.now(); return client;
  }
  private push(client: RpcClient, event: HostEvent): void {
    if (!this.clients.has(client.id)) return;
    if (event.type === 'remote' && event.event.type === 'terminal' && event.event.event.type === 'control') {
      const peerId = event.event.peerId, tabId = event.event.event.tabId;
      client.events = client.events.filter(queued => queued.type !== 'remote' || queued.event.type !== 'terminal' || queued.event.peerId !== peerId || queued.event.event.tabId !== tabId);
      client.bytes = client.events.reduce((sum, queued) => sum + Buffer.byteLength(JSON.stringify(queued)), 0);
    }
    const bytes = Buffer.byteLength(JSON.stringify(event));
    if (client.events.length >= 1024 || client.bytes + bytes > 1024 * 1024) {
      // Disconnect the view rather than silently lose a credited terminal frame.
      this.drop(client.id); return;
    }
    client.events.push(event); client.bytes += bytes; client.notify?.();
  }
  broadcast(event: Exclude<HostEvent, { type: 'terminal' }>): void { for (const client of this.clients.values()) this.push(client, event); }
  private purge(client: RpcClient, tabId: string): void {
    client.events = client.events.filter(event => event.type !== 'terminal' || event.event.tabId !== tabId);
    client.bytes = client.events.reduce((sum, event) => sum + Buffer.byteLength(JSON.stringify(event)), 0);
  }
  async poll(client: RpcClient, signal: AbortSignal): Promise<{ events: HostEvent[] }> {
    if (client.polling) throw new Error('Only one event poll per client is allowed.');
    client.polling = true;
    try {
      if (!client.events.length && !signal.aborted) await new Promise<void>(resolve => {
        const done = () => { clearTimeout(timer); signal.removeEventListener('abort', done); client.notify = undefined; resolve(); };
        const timer = setTimeout(done, 20_000); client.notify = done; signal.addEventListener('abort', done, { once: true });
      });
      this.get(client.id);
      const events = client.events; client.events = []; client.bytes = 0; return { events };
    } finally { client.polling = false; }
  }
  drop(id: string): void {
    const client = this.clients.get(id); if (!client) return;
    this.clients.delete(id); this.onDrop?.(id); client.delivery.dispose(); this.service.clearRemoteVisibleSessions(id); client.notify?.();
    client.unsubscribeRemote?.();
    if (client.remote) { const cleanup = client.remote.dispose().catch(() => {}); this.cleanup.add(cleanup); void cleanup.finally(() => this.cleanup.delete(cleanup)); }
  }
  async dispose(): Promise<void> { clearInterval(this.timer); for (const id of this.clients.keys()) this.drop(id); await Promise.allSettled(this.cleanup); }
}
