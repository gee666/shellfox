import { z } from 'zod';
import { remoteNetworkPeerSchema } from '../../shared/remote-schemas';
import { ProxyResponseError, proxyRequest } from './transport';
import type { DiscoveryCache } from './discovery-cache';

export const DISCOVERY_POSITIVE_TTL = 5 * 60_000;
export const DISCOVERY_NEGATIVE_TTL = 30_000;
export const DISCOVERY_TIMEOUT = 2000;
const signature = z.object({ ok: z.literal(true), value: z.object({ service: z.literal('shellfox'), protocolVersion: z.literal(1) }).strict() }).strict();
const legacySignature = z.object({ ok: z.literal(false), error: z.object({ code: z.literal('VALIDATION'), message: z.literal('Remote request could not be completed.'), retryable: z.literal(true) }).strict() }).strict();
type NetworkPeer = z.infer<typeof remoteNetworkPeerSchema>;
interface Entry { confirmed: boolean; next: number }
interface Probe { id: string; epoch: number; signal: AbortSignal }

/** The same identity-free service check as mobile. Never pair or send a client ID. */
export async function probeShellfoxHost(proxyUrl: string, secret: string, peerId: string, signal: AbortSignal): Promise<boolean> {
  const deadline = AbortSignal.any([signal, AbortSignal.timeout(DISCOVERY_TIMEOUT)]);
  try { return signature.safeParse(await proxyRequest(proxyUrl, secret, peerId, '/v1/discover', {}, deadline, 4096)).success; }
  catch (error) {
    if (!(error instanceof ProxyResponseError) || error.status !== 404) return false;
    // Older hosts reject discovery before their side-effect-free validation path.
    try { return legacySignature.safeParse(await proxyRequest(proxyUrl, secret, peerId, '/v1/request', {}, deadline, 4096)).success; }
    catch { return false; }
  }
}

/** Confirm hosts behind the current helper-authenticated, same-owner peer map. */
export class RemoteDiscovery {
  private readonly entries = new Map<string, Entry>();
  private network = new Map<string, NetworkPeer>();
  private readonly pending = new Set<string>();
  private queue: Probe[] = [];
  private active = 0;
  private epoch = 0;
  private abort = new AbortController();
  constructor(private readonly probe: (peerId: string, signal: AbortSignal) => Promise<boolean>, private readonly changed: () => void, private readonly now: () => number = Date.now, private readonly cache?: DiscoveryCache) {}
  async load(): Promise<void> {
    const ids = await this.cache?.load() ?? [];
    for (const id of ids) this.entries.set(id, { confirmed: true, next: 0 });
  }
  private save(): void {
    const ids = [...this.entries].filter(([, entry]) => entry.confirmed).map(([id]) => id).slice(0, 1000);
    void Promise.resolve().then(() => this.cache?.save(ids)).catch(() => {});
  }
  update(peers: NetworkPeer[], retainedIds: Iterable<string> = []): void {
    this.network = new Map(peers.map(peer => [peer.id, peer]));
    const retained = new Set(retainedIds);
    let removed = false;
    for (const [id, entry] of this.entries) if (!this.network.has(id) && !retained.has(id)) { this.entries.delete(id); removed ||= entry.confirmed; }
    if (removed) this.save();
    for (const peer of peers) {
      if (!peer.online || this.pending.has(peer.id)) continue;
      const entry = this.entries.get(peer.id);
      if (entry && this.now() < entry.next) continue;
      this.pending.add(peer.id);
      this.queue.unshift({ id: peer.id, epoch: this.epoch, signal: this.abort.signal });
    }
    this.drain();
  }
  /** A validated Shellfox pair is stronger evidence than an optional discovery probe. */
  confirm(id: string): void {
    this.entries.set(id, { confirmed: true, next: this.now() + DISCOVERY_POSITIVE_TTL });
    this.save(); this.changed();
  }
  confirmed(id: string): boolean { return this.entries.get(id)?.confirmed === true; }
  peers(): NetworkPeer[] { return [...this.network.values()].filter(peer => this.confirmed(peer.id)); }
  /** Fence replies after logout/disable and keep no old network identities visible. */
  clear(): void {
    this.abort.abort(); this.abort = new AbortController(); this.epoch++;
    this.queue = []; this.pending.clear(); this.network.clear();
    for (const entry of this.entries.values()) entry.next = 0;
  }
  private drain(): void {
    while (this.active < 4 && this.queue.length) {
      const job = this.queue.shift(); if (!job) break;
      if (!this.network.get(job.id)?.online || job.signal.aborted) { this.pending.delete(job.id); continue; }
      this.active++;
      void this.run(job);
    }
  }
  private async run(job: Probe): Promise<void> {
    let confirmed = false;
    try { confirmed = await this.probe(job.id, job.signal); } catch { /* Network failures are negative discovery, not host errors. */ }
    finally {
      if (job.epoch === this.epoch) {
        this.pending.delete(job.id);
        if (!job.signal.aborted && this.network.has(job.id)) {
          const previous = this.entries.get(job.id);
          // A previously confirmed host remains a known host while offline or temporarily unreachable.
          this.entries.set(job.id, { confirmed: confirmed || previous?.confirmed === true, next: this.now() + (confirmed ? DISCOVERY_POSITIVE_TTL : DISCOVERY_NEGATIVE_TTL) });
          if (confirmed && !previous?.confirmed) this.save();
          this.changed();
        }
      }
      this.active--; this.drain();
    }
  }
}
