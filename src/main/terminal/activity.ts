import type { TerminalEvent } from '../../shared/contracts';

export const TERMINAL_QUIET_MS = 2500;
export const TERMINAL_ECHO_MS = 250;
/** One instance per owned PTY generation. Output remains independent of renderer views. */
export class TerminalActivity {
  private busy = false;
  private inputAt = -Infinity;
  private timer?: ReturnType<typeof setTimeout>;
  constructor(private readonly tabId: string, private readonly emit: (event: TerminalEvent) => void) {}
  input(): void { this.inputAt = Date.now(); }
  output(): void {
    if (Date.now() - this.inputAt < TERMINAL_ECHO_MS) return;
    if (this.timer) clearTimeout(this.timer);
    if (!this.busy) { this.busy = true; this.emit({ type: 'activity', tabId: this.tabId, busy: true }); }
    this.timer = setTimeout(() => this.stop(), TERMINAL_QUIET_MS);
    this.timer.unref?.();
  }
  stop(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = undefined;
    if (this.busy) { this.busy = false; this.emit({ type: 'activity', tabId: this.tabId, busy: false }); }
  }
}
