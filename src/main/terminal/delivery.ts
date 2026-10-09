import type { Result, TerminalAttachmentDto, TerminalEvent } from '../../shared/contracts';
import { failure, success } from '../../shared/contracts';
import { requestSchemas, terminalEventSchema } from '../../shared/schemas';
const CREDIT = 128 * 1024;
interface Attachment { generation: string; sent: number; acked: number; replayPending: number | null; pending: { sequence: number; bytes: number }[]; pendingBytes: number; blocked: boolean; exit?: Extract<TerminalEvent, { type: 'exit' }> }
/** Per renderer view. No output queue here: unsent data lives only in the bounded PTY replay. */
export class TerminalDelivery {
  private readonly attachments = new Map<string, Attachment>();
  constructor(private readonly attach: (input: { tabId: string; generation?: string; afterSequence?: number }) => Result<TerminalAttachmentDto>, private readonly send: (event: TerminalEvent) => void) {}
  attached(snapshot: TerminalAttachmentDto): Result<{ attached: true }> {
    const current = this.attachments.get(snapshot.tabId);
    if (current?.generation === snapshot.generation && (current.pending.length || current.replayPending !== null)) return failure('RETRY_CONFIRM_REQUIRED', 'Consume and acknowledge the previous terminal output, or explicitly detach the view before reattaching.');
    if (!current && this.attachments.size >= 16) return failure('VALIDATION', 'At most 16 terminal views may be attached. Detach unused views first.');
    this.attachments.set(snapshot.tabId, { generation: snapshot.generation, sent: snapshot.lastSequence, acked: 0, replayPending: snapshot.chunks.length ? snapshot.lastSequence : null, pending: [], pendingBytes: 0, blocked: false });
    return success({ attached: true });
  }
  detach(input: { tabId: string; generation: string }): Result<{ detached: true }> {
    if (!requestSchemas.detachTerminal.safeParse(input).success) return failure('VALIDATION', 'Invalid detach request.');
    const a = this.attachments.get(input.tabId);
    if (a && a.generation !== input.generation) return failure('TARGET_LOST', 'Stale terminal view generation.');
    this.attachments.delete(input.tabId); return success({ detached: true });
  }
  acknowledge(input: { tabId: string; generation: string; sequence: number }): Result<{ acknowledged: true }> {
    if (!requestSchemas.acknowledgeTerminal.safeParse(input).success) return failure('VALIDATION', 'Invalid terminal acknowledgement.');
    const a = this.attachments.get(input.tabId);
    if (!a || a.generation !== input.generation) return failure('TARGET_LOST', 'This terminal view is no longer attached.');
    if (input.sequence < a.acked || input.sequence > a.sent) return failure('VALIDATION', 'Acknowledgement is outside the delivered sequence range.');
    a.acked = input.sequence;
    let consumed = 0;
    while (consumed < a.pending.length && a.pending[consumed].sequence <= input.sequence) a.pendingBytes -= a.pending[consumed++].bytes;
    if (consumed) a.pending.splice(0, consumed);
    if (a.replayPending !== null && input.sequence >= a.replayPending) a.replayPending = null;
    this.pump(input.tabId, a); return success({ acknowledged: true });
  }
  event(raw: TerminalEvent): void {
    const parsed = terminalEventSchema.safeParse(raw); if (!parsed.success) return;
    const e = parsed.data;
    if (e.type === 'activity') { this.send(e); return; }
    const a = this.attachments.get(e.tabId);
    if (!a || a.generation !== e.generation) return;
    if (e.type === 'error') { this.send(e); return; }
    if (e.type === 'exit') a.exit = e;
    // The usual live path already has the next chunk. Avoid asking the backend
    // to copy and validate a replay suffix on every PTY output event.
    if (e.type === 'data' && !a.blocked && a.replayPending === null && e.sequence === a.sent + 1) {
      const bytes = Buffer.byteLength(e.data);
      if (a.pending.length < 256 && a.pendingBytes + bytes <= CREDIT) {
        a.pending.push({ sequence: e.sequence, bytes }); a.pendingBytes += bytes; a.sent = e.sequence;
        this.send(e);
      }
      // Credit exhaustion is recovered by ACK/pump, not by dropping PTY output.
      return;
    }
    this.pump(e.tabId, a);
  }
  private pump(tabId: string, a: Attachment): void {
    if (a.blocked || a.replayPending !== null) return;
    const replay = this.attach({ tabId, generation: a.generation, afterSequence: a.sent });
    if (!replay.ok) return;
    if (replay.value.truncated || replay.value.generation !== a.generation) {
      a.blocked = true;
      this.send({ type: 'error', tabId, generation: a.generation, error: { code: 'NATIVE_UNAVAILABLE', message: 'Terminal output exceeded the view replay window. Reattach to the current bounded output; older output was discarded.', retryable: true } });
      return;
    }
    for (const e of replay.value.chunks) {
      const size = Buffer.byteLength(e.data);
      if (a.pendingBytes + size > CREDIT || a.pending.length >= 256) break;
      a.pending.push({ sequence: e.sequence, bytes: size }); a.pendingBytes += size; a.sent = e.sequence; this.send(e);
    }
    if (a.exit && a.sent >= a.exit.lastSequence) { this.send(a.exit); a.exit = undefined; }
  }
  dispose(): void { this.attachments.clear(); }
}
