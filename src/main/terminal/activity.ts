import type { TerminalEvent } from '../../shared/contracts';

export const TERMINAL_QUIET_MS = 2500;
export const TERMINAL_ECHO_MS = 250;
/** Cursor moves/saves and synchronized-output/cursor-visibility toggles redraw nothing, so idle TUIs emitting them are not working. */
const IDLE_SEQUENCES = /\x1b(?:\[\?(?:2026|25)[hl]|\[[0-9;]*[HfABCDEFGdsu]|[78])/g;
/** An escape sequence cut by a chunk boundary; held until the next chunk so a split idle sequence is still recognised. */
const PARTIAL_ESCAPE = /\x1b(?:\[[?0-9;]{0,16})?$/;
/** One instance per owned PTY generation. Output remains independent of renderer views. */
export class TerminalActivity {
  private busy = false;
  private inputAt = -Infinity;
  private pending = '';
  private timer?: ReturnType<typeof setTimeout>;
  constructor(private readonly tabId: string, private readonly emit: (event: Extract<TerminalEvent, { type: 'activity' }>) => void) {}
  get isBusy(): boolean { return this.busy; }
  input(): void { this.inputAt = Date.now(); }
  /** Raw PTY data; only chunks carrying more than idle cursor/sync boilerplate count as activity. */
  output(data: string): void {
    const text = this.pending + data;
    const partial = PARTIAL_ESCAPE.exec(text);
    this.pending = partial?.[0] ?? '';
    if (!text.slice(0, partial?.index).replace(IDLE_SEQUENCES, '')) return;
    if (Date.now() - this.inputAt < TERMINAL_ECHO_MS) return;
    if (this.timer) clearTimeout(this.timer);
    if (!this.busy) { this.busy = true; this.emit({ type: 'activity', tabId: this.tabId, busy: true }); }
    this.timer = setTimeout(() => this.stop(), TERMINAL_QUIET_MS);
    this.timer.unref?.();
  }
  stop(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = undefined;
    this.pending = '';
    if (this.busy) { this.busy = false; this.emit({ type: 'activity', tabId: this.tabId, busy: false }); }
  }
}
