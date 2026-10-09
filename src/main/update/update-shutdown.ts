import type { InstallHandoff } from './handoff';

/** Keep the service usable until the helper acknowledges an installation. */
export class UpdateShutdown {
  private busy = false;
  private accepted = false;
  private pending: InstallHandoff | null = null;
  private resume: (() => void) | null = null;
  constructor(private readonly options: {
    confirm(onQuit: boolean): boolean;
    closeTerminals(): Promise<() => void>;
    finish(): Promise<void>;
  }) {}
  async cancelPending(): Promise<void> {
    if (this.accepted || this.pending?.committed) { this.accepted = true; return; }
    // Do not resume mutations until cancellation is confirmed. Retain both
    // handles if cancellation fails, so a retry or normal quit can try again.
    await this.pending?.cancel();
    if (this.pending?.committed) { this.accepted = true; return; }
    this.pending = null;
    this.resume?.(); this.resume = null;
  }
  async run(start: () => Promise<InstallHandoff>, onQuit = false): Promise<boolean> {
    if (this.busy || this.accepted) return false;
    this.busy = true;
    try {
      await this.cancelPending();
      if (this.accepted || !this.options.confirm(onQuit)) return false;
      this.pending = await start();
      this.resume = await this.options.closeTerminals();
      await this.pending.commit();
      this.accepted = true;
      await this.options.finish();
      return true;
    } catch (error) {
      await this.cancelPending();
      throw error;
    } finally { this.busy = false; }
  }
}
