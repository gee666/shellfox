import type { Result } from '../shared/contracts';
import { success } from '../shared/contracts';
import type { RepositoryPort, SessionRecord } from './models';

export const LIFECYCLE_INTERVAL_MS = 60_000;
const HOUR_MS = 3_600_000;

/** Keeps high-volume activity in memory. Only the minute sweep writes it to storage. */
export class SessionLifecycle {
  private readonly activity = new Map<string, number>();
  private timer?: ReturnType<typeof setInterval>;
  private sweeping = false;
  private reportedFailure = false;
  constructor(
    private readonly repository: RepositoryPort,
    private readonly serialize: (id: string, action: () => Promise<Result<true>>) => Promise<Result<true>>,
    private readonly remove: (session: SessionRecord) => Promise<Result<unknown>>,
    private readonly changed: () => void,
    private readonly refreshActivity: () => Promise<void> = async () => {},
    private readonly canExpire: (sessionId: string) => boolean = () => true,
  ) {}
  start(): void {
    if (this.timer) return;
    this.timer = setInterval(() => {
      void this.sweep().then(() => { this.reportedFailure = false; }, error => {
        if (!this.reportedFailure) console.warn('Session lifecycle sweep failed; it will retry.', error);
        this.reportedFailure = true;
      });
    }, LIFECYCLE_INTERVAL_MS);
    this.timer.unref?.();
  }
  touch(id: string): void { this.activity.set(id, Date.now()); }
  forget(id: string): void { this.activity.delete(id); }
  private flush(session: SessionRecord): boolean {
    const at = this.activity.get(session.id);
    if (at === undefined) return false;
    if (at > Date.parse(session.updatedAt)) {
      session.updatedAt = new Date(at).toISOString();
      this.repository.saveSession(session);
    }
    if (this.activity.get(session.id) === at) this.activity.delete(session.id);
    return true;
  }
  private due(session: SessionRecord): boolean {
    const settings = this.repository.settings();
    const lastAction = Math.max(Date.parse(session.updatedAt), this.activity.get(session.id) ?? 0);
    const deadline = session.settledAt
      ? session.pinnedAt ? Infinity : Math.max(Date.parse(session.settledAt), lastAction) + settings.sessionDeleteHours * HOUR_MS
      : lastAction + settings.sessionSettleHours * HOUR_MS;
    return Date.now() >= deadline;
  }
  async sweep(): Promise<void> {
    if (this.sweeping) return;
    this.sweeping = true;
    try {
      let refresh: Promise<void> | undefined;
      for (const record of this.repository.sessions()) await this.serialize(record.id, async () => {
        let session = this.repository.session(record.id);
        if (!session) { this.forget(record.id); return success(true); }
        if (this.due(session)) {
          // Viewport polling misses hidden children. Refresh every owned root once
          // before expiry, then recheck actions received during that refresh.
          refresh ??= this.refreshActivity();
          await refresh;
          session = this.repository.session(record.id);
          if (!session) { this.forget(record.id); return success(true); }
        }
        const touched = this.flush(session);
        if (!this.due(session) || !this.canExpire(session.id)) {
          if (touched) this.changed();
          return success(true);
        }
        if (!session.settledAt) {
          // Settlement starts a fresh deletion grace period, even after a long shutdown.
          session.settledAt = new Date().toISOString();
          this.repository.saveSession(session); this.changed();
        } else {
          await this.remove(session);
        }
        return success(true);
      });
    } finally { this.sweeping = false; }
  }
  stop(): void {
    if (this.timer) clearInterval(this.timer);
    this.timer = undefined;
  }
  flushActivity(): void {
    for (const id of this.activity.keys()) {
      const session = this.repository.session(id);
      if (session) this.flush(session); else this.forget(id);
    }
  }
}

/** Poll timestamps and diagnostic wording do not constitute process activity. */
export function processActivityKey(item: { root: string; health: string; agents: number }): string {
  return `${item.root}/${item.health}/${item.agents}`;
}
