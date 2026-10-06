import { Terminal } from '@xterm/headless';
import { SerializeAddon } from '@xterm/addon-serialize';

/** Matches the renderer's scrollback so a restored view keeps the same history depth. */
export const MIRROR_SCROLLBACK = 3000;
const SCROLLBACK_STEPS = [MIRROR_SCROLLBACK, 1500, 600, 200, 50, 0];

export interface ScreenSnapshot {
  /** Highest PTY output sequence already parsed into this state. */
  sequence: number;
  cols: number;
  rows: number;
  /** Escape-sequence stream that rebuilds history, the visible screen, SGR state, cursor, modes and the active buffer. */
  data: string;
}

/**
 * Headless VT state of one PTY, kept in the main process next to the raw replay ring.
 *
 * Full-screen TUIs paint once and then only send cursor-addressed diffs. A byte tail of that
 * stream is therefore not a screen: replaying it into a fresh terminal leaves every row that
 * was not rewritten recently blank. The mirror parses the whole stream so a view that cannot
 * be rebuilt from the ring can be restored from a serialized screen instead.
 */
export class ScreenMirror {
  private readonly term: Terminal;
  private readonly serializer = new SerializeAddon();
  private parsed = 0;
  private disposed = false;
  private broken = false;
  /**
   * xterm's public `write` parses on later timer ticks, so a burst leaves the mirror behind the
   * replay ring exactly when a snapshot is needed. The input handler's `parse` is what that queue
   * calls; using it directly keeps the mirror in lockstep with the PTY. Missing in some future
   * xterm version: fall back to the queue (the snapshot is then skipped while it lags).
   */
  private readonly parseNow: ((data: string) => unknown) | null;
  constructor(cols: number, rows: number) {
    this.term = new Terminal({ cols, rows, scrollback: MIRROR_SCROLLBACK, allowProposedApi: true, convertEol: false, logLevel: 'off' });
    const handler = (this.term as unknown as { _core?: { _inputHandler?: { parse?: (data: string) => unknown } } })._core?._inputHandler;
    this.parseNow = typeof handler?.parse === 'function' ? (data: string) => handler.parse!(data) : null;
    // The addon is typed for the DOM terminal but only uses the shared buffer/mode API.
    this.term.loadAddon(this.serializer as unknown as Parameters<Terminal['loadAddon']>[0]);
  }
  get sequence(): number { return this.parsed; }
  /** `sequence` advances once the chunk has been parsed into the mirror. */
  write(sequence: number, data: string): void {
    if (this.disposed || this.broken) return;
    if (this.parseNow) {
      // A parser failure leaves the state undefined; refuse snapshots rather than serve a wrong screen.
      try { this.parseNow(data); this.parsed = sequence; } catch { this.broken = true; }
      return;
    }
    this.term.write(data, () => { this.parsed = sequence; });
  }
  /** Ordered with output: output written before the resize is parsed at the old size. */
  resize(cols: number, rows: number): void {
    if (this.disposed || this.broken) return;
    if (this.parseNow) { if (this.term.cols !== cols || this.term.rows !== rows) this.term.resize(cols, rows); return; }
    this.term.write('', () => { if (!this.disposed && (this.term.cols !== cols || this.term.rows !== rows)) this.term.resize(cols, rows); });
  }
  /** `maxBytes` bounds the UTF-8 size; history is shortened before the visible screen is. */
  snapshot(maxBytes: number): ScreenSnapshot | null {
    if (this.disposed || this.broken) return null;
    for (const scrollback of SCROLLBACK_STEPS) {
      const data = this.serializer.serialize({ scrollback }) + this.tail();
      if (Buffer.byteLength(data) <= maxBytes) return { sequence: this.parsed, cols: this.term.cols, rows: this.term.rows, data };
    }
    return null;
  }
  /**
   * State the serializer omits or restores imprecisely: scroll region, SGR mouse encoding, cursor
   * visibility and the exact cursor cell (it uses relative moves, which are off by one while a
   * wrap is pending). The internals are read defensively; losing them only degrades fidelity.
   */
  private tail(): string {
    let out = '';
    const buffer = this.term.buffer.active;
    try {
      const core = (this.term as unknown as { _core: { buffer: { scrollTop: number; scrollBottom: number }; coreMouseService?: { activeProtocol?: string; activeEncoding?: string }; coreService?: { isCursorHidden?: boolean } } })._core;
      const { scrollTop, scrollBottom } = core.buffer;
      if (scrollTop > 0 || scrollBottom < this.term.rows - 1) out += `\x1b[${scrollTop + 1};${scrollBottom + 1}r`;
      const encoding = core.coreMouseService?.activeEncoding;
      if (core.coreMouseService?.activeProtocol && core.coreMouseService.activeProtocol !== 'NONE') {
        if (encoding === 'SGR') out += '\x1b[?1006h'; else if (encoding === 'UTF8') out += '\x1b[?1005h'; else if (encoding === 'SGR_PIXELS') out += '\x1b[?1016h';
      }
      out += core.coreService?.isCursorHidden ? '\x1b[?25l' : '\x1b[?25h';
    } catch { /* Fidelity only. */ }
    // DECSTBM homes the cursor, so the absolute position goes last.
    return out + `\x1b[${buffer.cursorY + 1};${buffer.cursorX + 1}H`;
  }
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    try { this.serializer.dispose(); this.term.dispose(); } catch { /* A mirror must never affect PTY ownership. */ }
  }
}
