import type { NetworkStatus } from './network';

export const NETWORK_RECOVERY_GRACE_MS = 15_000;

/** Keep expected startup/resume failures quiet while automatic recovery can still help. */
export class NetworkRecovery {
  private failedAt: number | null = null;
  reset(): void { this.failedAt = null; }
  get pending(): boolean { return this.failedAt !== null; }
  failure(message: string, workingConnection: boolean): NetworkStatus {
    this.failedAt ??= Date.now();
    if (workingConnection) return { status: 'online', loginUrl: null, error: null };
    return Date.now() - this.failedAt < NETWORK_RECOVERY_GRACE_MS
      ? { status: 'starting', loginUrl: null, error: null }
      : { status: 'error', loginUrl: null, error: message };
  }
}
