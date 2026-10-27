import type { AppError } from '../../shared/contracts';
import type { RemotePeer } from '../../shared/remote-contracts';
import type { z } from 'zod';
import type { peerIdentitySchema } from './auth';

type Peer = z.infer<typeof peerIdentitySchema>;
interface Attempts { seen: Map<string, number>; denied: Map<string, number> }
export const REMOTE_APPROVAL_TTL_MS = 60 * 60 * 1000;
const DENIED_TTL = 300_000, MAX_PEERS = 1000, MAX_ATTEMPTS = 256;
const required = (): AppError => ({ code: 'APPROVAL_REQUIRED', message: 'Waiting for approval in Shellfox on the host.', retryable: true });
const denied = (): AppError => ({ code: 'ACCESS_DENIED', message: 'The host declined, cancelled or revoked this connection. Request access again to start a new attempt.', retryable: false });

/** Attempt tombstones stop cancelled requests returning. Device approval remains independent. */
export class RemoteApprovals {
  readonly pending = new Map<string, Pick<RemotePeer, 'id' | 'hostname' | 'addresses' | 'online'>>();
  private readonly attempts = new Map<string, Attempts>();
  constructor(private readonly approved: (id: string) => boolean, private readonly changed: () => void, private readonly now: () => number = Date.now) {}
  private record(peerId: string): Attempts {
    const record = this.attempts.get(peerId) ?? { seen: new Map<string, number>(), denied: new Map<string, number>() };
    this.attempts.delete(peerId); this.attempts.set(peerId, record);
    while (this.attempts.size > MAX_PEERS) {
      const oldest = [...this.attempts.keys()].find(id => !this.pending.has(id));
      if (oldest === undefined) break;
      this.attempts.delete(oldest);
    }
    return record;
  }
  private block(record: Attempts, clientId: string): void {
    record.seen.delete(clientId); record.denied.delete(clientId); record.denied.set(clientId, this.now() + DENIED_TTL);
    while (record.denied.size > MAX_ATTEMPTS) { const oldest = record.denied.keys().next().value; if (oldest === undefined) break; record.denied.delete(oldest); }
  }
  authorize(peer: Peer, clientId: string): AppError | null {
    const approved = this.approved(peer.id), now = this.now();
    if (!approved && !this.pending.has(peer.id) && this.pending.size >= 100) this.expire();
    const record = this.record(peer.id);
    const until = record.denied.get(clientId);
    if (until !== undefined && until > now) return denied();
    record.denied.delete(clientId);
    if (!approved && !this.pending.has(peer.id) && this.pending.size >= 100) {
      return { code: 'VALIDATION', message: 'The host has too many pending access requests. Retry after an existing request is resolved.', retryable: true };
    }
    record.seen.delete(clientId); record.seen.set(clientId, now);
    while (record.seen.size > MAX_ATTEMPTS) { const oldest = record.seen.keys().next().value; if (oldest === undefined) break; record.seen.delete(oldest); }
    if (approved) return null;
    if (!this.pending.has(peer.id)) {
      this.pending.set(peer.id, { ...peer, addresses: [], online: true }); this.changed();
    }
    return required();
  }
  approve(peerId: string): void { this.pending.delete(peerId); }
  deny(peerId: string): void {
    const record = this.record(peerId);
    for (const id of [...record.seen.keys()]) this.block(record, id);
    this.pending.delete(peerId);
  }
  withdraw(peerId: string, clientId: string): void {
    const record = this.record(peerId); this.block(record, clientId);
    if (!record.seen.size && this.pending.delete(peerId)) this.changed();
  }
  expire(): void {
    const now = this.now(); let changed = false;
    for (const [peerId, record] of this.attempts) {
      for (const [id, until] of record.denied) if (until <= now) record.denied.delete(id);
      for (const [id, touched] of record.seen) if (now - touched > REMOTE_APPROVAL_TTL_MS) {
        // Silence is not a rejection. A lost reply or offline client may retry this UUID.
        record.seen.delete(id);
      }
      if (!record.seen.size && this.pending.delete(peerId)) changed = true;
      if (!record.seen.size && !record.denied.size) this.attempts.delete(peerId);
    }
    if (changed) this.changed();
  }
}
