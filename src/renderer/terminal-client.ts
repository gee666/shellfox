import { Terminal, type ITheme } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import type { AppError, TerminalApi, TerminalAttachmentDto, TerminalEvent } from '../shared/contracts';
import { request } from './api';
import { DEFAULT_ACCENT, DEFAULT_BACKGROUND, derivePalette } from './theme';

const DEFAULT_THEME: ITheme = derivePalette(DEFAULT_ACCENT, DEFAULT_BACKGROUND).terminal;

// Activity is metadata-only and has no terminal generation or output sequence.
type DeliveryEvent = Exclude<TerminalEvent, { type: 'activity' }>;
const MAX_BUFFER = 512 * 1024;
// A valid attach can contain 8192 replay chunks; live delivery adds at most
// 256 outstanding chunks. Object limits must not reject that bounded replay.
const MAX_QUEUED_CHUNKS = 8192 + 256;
const WRITE_BATCH_BYTES = 16 * 1024;
const REPAINT_NUDGE_MS = 80;
const encoder = new TextEncoder();
export interface TerminalSurface {
  element: HTMLElement;
  write(data: string, done: () => void): void;
  reset(): void;
  fit(): { cols: number; rows: number } | null;
  /** Sets the grid before any output is parsed, so a snapshot is replayed at the size it was taken at. */
  resize?(cols: number, rows: number): void;
  focus(): void;
  setInput(enabled: boolean): void;
  setTheme?(theme: ITheme): void;
  dispose(): void;
}
/** Clipboard access goes through the main process; the renderer has no clipboard permissions. */
export interface SurfaceClipboard {
  copy(text: string): void;
  /** Resolves with clipboard text, or null when nothing can be pasted. */
  read(): Promise<string | null>;
}
export type SurfaceFactory = (input: (data: string) => void, theme?: ITheme, clipboard?: SurfaceClipboard) => TerminalSurface;
/** Ctrl+Shift+C / Ctrl+Shift+V (KeyboardEvent.code, so it is keyboard-layout independent). */
export function clipboardShortcut(ev: KeyboardEvent): 'copy' | 'paste' | null {
  if (!ev.ctrlKey || !ev.shiftKey || ev.altKey || ev.metaKey) return null;
  return ev.code === 'KeyC' ? 'copy' : ev.code === 'KeyV' ? 'paste' : null;
}
export const createTerminalSurface: SurfaceFactory = (input, theme = DEFAULT_THEME, clipboard) => {
  const element = document.createElement('div');
  element.className = 'terminal-surface';
  const terminal = new Terminal({
    allowProposedApi: true, scrollback: 3000, fontSize: 14,
    fontFamily: "'Cascadia Code', Consolas, 'DejaVu Sans Mono', monospace",
    cursorBlink: true, convertEol: false, disableStdin: true,
    // Preserve application RGB choices; bold must not remap ANSI palette colors.
    minimumContrastRatio: 1, drawBoldTextInBrightColors: false,
    overviewRuler: { width: 8 },
    theme,
    linkHandler: { activate: () => {}, allowNonHttpProtocols: false },
  });
  const fit = new FitAddon();
  terminal.loadAddon(fit);
  // Shell output must not read/write the OS clipboard or open hyperlinks.
  terminal.parser.registerOscHandler(52, () => true);
  terminal.parser.registerOscHandler(8, () => true);
  terminal.onData(input);
  if (clipboard) terminal.attachCustomKeyEventHandler(ev => {
    const action = clipboardShortcut(ev);
    if (!action) return true;
    // Swallow keydown/keypress/keyup so xterm never sends ^C/^V, and stop Chromium's
    // own paste-as-plain-text from firing a second paste event.
    if (ev.type !== 'keydown') return false;
    ev.preventDefault();
    if (action === 'copy') { if (terminal.hasSelection()) clipboard.copy(terminal.getSelection()); }
    // paste() applies bracketed-paste mode and newline normalization, then emits onData.
    else void clipboard.read().then(text => { if (text) terminal.paste(text); });
    return false;
  });
  let opened = false;
  return {
    element,
    write: (data, done) => terminal.write(data, done), reset: () => terminal.reset(),
    fit() {
      if (!element.isConnected || element.clientWidth < 20 || element.clientHeight < 20) return null;
      if (!opened) { terminal.open(element); opened = true; }
      const size = fit.proposeDimensions();
      if (!size || !Number.isFinite(size.cols) || !Number.isFinite(size.rows)) return null;
      const cols = Math.max(2, Math.min(500, Math.floor(size.cols)));
      const rows = Math.max(2, Math.min(500, Math.floor(size.rows)));
      terminal.resize(cols, rows);
      terminal.refresh(0, terminal.rows - 1);
      return { cols, rows };
    },
    resize: (cols, rows) => { if (cols !== terminal.cols || rows !== terminal.rows) terminal.resize(cols, rows); },
    focus: () => { if (opened) terminal.focus(); },
    setInput: enabled => { terminal.options.disableStdin = !enabled; },
    setTheme: next => { terminal.options.theme = next; },
    dispose: () => { terminal.dispose(); element.remove(); },
  };
};

export interface TerminalViewState {
  generation: string | null;
  phase: 'connecting' | 'open' | 'closed' | 'unavailable';
  error: AppError | null;
  warning: string | null;
  exitCode: number | null;
}
interface Entry {
  id: string; surface: TerminalSurface; state: TerminalViewState; listeners: Set<() => void>;
  host: HTMLElement | null; visible: boolean; used: number; sequence: number;
  pending: Map<number, Extract<TerminalEvent, { type: 'data' }>>; pendingBytes: number;
  early: DeliveryEvent[]; earlyBytes: number; retired: Set<string>;
  attaching: boolean; attachToken: number; replayAgain: boolean; fullReplay: boolean;
  writes: { sequence: number; data: string; bytes: number }[]; writeBytes: number; writing: boolean; epoch: number; inputEpoch: number;
  consumed: number; nextAck: number; ackTask: Promise<void> | null; detachQueue: Promise<boolean>;
  inputQueue: Promise<void>; inputBytes: number;
  resized: string; resizing: boolean; nextSize: { cols: number; rows: number } | null;
  // Set when a view was rebuilt from a partial raw tail: the app is asked to repaint itself.
  repaint: boolean;
}

// Metadata snapshots never carry terminal output. This registry owns bounded
// screen buffers independently of React session/settings component lifetimes.
export class TerminalRegistry {
  private entries = new Map<string, Entry>();
  private unsubscribe?: () => void;
  private shutdown?: ReturnType<typeof setTimeout>;
  private active = false;
  private theme: ITheme | undefined;
  constructor(private api: TerminalApi | null, private factory: SurfaceFactory = createTerminalSurface) {}

  start() {
    clearTimeout(this.shutdown);
    if (this.active) return;
    if (!this.api) return;
    this.active = true;
    try { this.unsubscribe = this.api.subscribeTerminal(event => { if ('generation' in event) this.receive(event); }); }
    catch { this.active = false; }
    for (const entry of this.entries.values()) void this.attach(entry);
  }
  stop() {
    // StrictMode's immediate cleanup/remount must not destroy retained screens.
    this.shutdown = setTimeout(() => this.dispose(), 0);
  }
  dispose() {
    clearTimeout(this.shutdown);
    this.active = false;
    this.unsubscribe?.(); this.unsubscribe = undefined;
    for (const entry of this.entries.values()) { if (entry.host) this.detach(entry); entry.attachToken++; entry.epoch++; entry.inputEpoch++; entry.surface.dispose(); }
    this.entries.clear();
  }
  private ensure(id: string): Entry {
    const existing = this.entries.get(id);
    if (existing) return existing;
    const entry: Entry = {
      id, surface: this.factory(data => { if (entry.inputEpoch === 0) this.input(entry, data); }, this.theme, this.clipboard(() => entry)),
      state: { generation: null, phase: 'connecting', error: null, warning: null, exitCode: null },
      listeners: new Set(), host: null, visible: false, used: Date.now(), sequence: 0,
      pending: new Map(), pendingBytes: 0, early: [], earlyBytes: 0, retired: new Set(), attaching: false,
      attachToken: 0, replayAgain: false, fullReplay: false,
      writes: [], writeBytes: 0, writing: false, epoch: 0, inputEpoch: 0,
      consumed: 0, nextAck: 0, ackTask: null, detachQueue: Promise.resolve(true),
      inputQueue: Promise.resolve(), inputBytes: 0, resized: '', resizing: false, nextSize: null, repaint: false,
    };
    this.entries.set(id, entry);
    return entry;
  }
  private clipboard(current: () => Entry): SurfaceClipboard | undefined {
    const api = this.api;
    if (!api?.copyText || !api.readClipboardText) return undefined;
    const fail = (error: AppError) => { const entry = current(); if (this.entries.get(entry.id) === entry) this.update(entry, { error }); };
    return {
      copy: text => { void request(() => api.copyText!({ text })).then(result => { if (!result.ok) fail(result.error); }); },
      read: async () => {
        const result = await request(() => api.readClipboardText!());
        if (!result.ok) { fail(result.error); return null; }
        return result.value.text;
      },
    };
  }
  /** Applies a derived palette to every existing terminal and to terminals created later. */
  setTheme(theme: ITheme) {
    this.theme = theme;
    for (const entry of this.entries.values()) entry.surface.setTheme?.(theme);
  }
  getState(id: string) { return this.ensure(id).state; }
  subscribe(id: string, listener: () => void) {
    const entry = this.ensure(id); entry.listeners.add(listener);
    return () => { entry.listeners.delete(listener); };
  }
  mount(id: string, host: HTMLElement, visible: boolean) {
    const entry = this.ensure(id);
    entry.host = host; entry.visible = visible; entry.used = Date.now();
    host.append(entry.surface.element);
    entry.surface.setInput(entry.state.phase === 'open');
    if (!this.active) this.update(entry, { phase: 'unavailable', error: { code: 'INTERNAL', message: 'Terminal connection unavailable. Refresh the terminal to reconnect.', retryable: true } });
    else void this.attach(entry);
    this.trim();
    return () => {
      if (entry.host === host) {
        entry.host = null; entry.visible = false; entry.surface.element.remove();
        this.detach(entry);
      }
    };
  }
  visibility(id: string, visible: boolean) {
    const entry = this.ensure(id); entry.visible = visible;
    entry.surface.setInput(entry.state.phase === 'open');
    if (visible) this.fit(id);
  }
  focus(id: string) {
    const entry = this.entries.get(id);
    if (entry?.visible && !document.querySelector('[role="dialog"]')) entry.surface.focus();
  }
  fit(id: string) {
    const entry = this.entries.get(id);
    if (!entry?.visible || !entry.host) return;
    const size = entry.surface.fit();
    if (entry.state.phase !== 'open' || !entry.state.generation) return;
    if (!size || !Number.isInteger(size.cols) || !Number.isInteger(size.rows) || size.cols < 2 || size.cols > 500 || size.rows < 2 || size.rows > 500) return;
    entry.nextSize = size;
    void this.resize(entry);
  }
  refresh(id: string) { if (!this.active) this.start(); const entry = this.ensure(id); void this.attach(entry); }
  private trim() {
    // Detached screens can be recovered from backend replay, never by relaunch.
    const detached = [...this.entries.values()].filter(e => !e.host && !e.listeners.size).sort((a, b) => a.used - b.used);
    while (this.entries.size > 24 && detached.length) {
      const entry = detached.shift()!; entry.attachToken++; entry.epoch++; entry.inputEpoch++; entry.surface.dispose(); this.entries.delete(entry.id);
    }
  }
  private update(entry: Entry, patch: Partial<TerminalViewState>) {
    entry.state = { ...entry.state, ...patch };
    // Keep parser-generated device replies working while hidden. The input
    // boundary separately rejects hidden keystrokes unless a write is parsing.
    entry.surface.setInput(entry.state.phase === 'open');
    entry.listeners.forEach(listener => listener());
  }
  private receive(event: DeliveryEvent) {
    const entry = this.entries.get(event.tabId);
    if (!entry || entry.retired.has(event.generation)) return;
    if (entry.attaching || !entry.state.generation || event.generation !== entry.state.generation) {
      entry.early.push(event);
      entry.earlyBytes += event.type === 'data' ? encoder.encode(event.data).length : 128;
      if (entry.earlyBytes > MAX_BUFFER || entry.early.length > MAX_QUEUED_CHUNKS) { entry.early = []; entry.earlyBytes = 0; entry.fullReplay = true; }
      if (!entry.attaching) void this.attach(entry);
      return;
    }
    this.apply(entry, event);
  }
  private apply(entry: Entry, event: DeliveryEvent) {
    if (event.generation !== entry.state.generation) return;
    if (event.type === 'error') {
      this.update(entry, { error: event.error });
      entry.fullReplay = true; void this.attach(entry); return;
    }
    if (event.type === 'exit') {
      this.update(entry, { phase: 'closed', exitCode: event.exitCode });
      if (event.lastSequence > entry.sequence) void this.attach(entry);
      return;
    }
    if (event.sequence <= entry.sequence || entry.pending.has(event.sequence)) return;
    entry.pending.set(event.sequence, event);
    entry.pendingBytes += encoder.encode(event.data).length;
    if (entry.pending.size > MAX_QUEUED_CHUNKS || entry.pendingBytes > MAX_BUFFER) {
      entry.fullReplay = true; void this.attach(entry); return;
    }
    while (entry.pending.has(entry.sequence + 1)) {
      const next = entry.pending.get(entry.sequence + 1)!;
      // Never move the received cursor past data that was not admitted. A full
      // queue deliberately discards this view through detach/full replay.
      if (!this.write(entry, next.data, next.sequence)) return;
      entry.pending.delete(next.sequence);
      entry.pendingBytes -= encoder.encode(next.data).length;
      entry.sequence = next.sequence;
    }
    if (entry.pending.size) void this.attach(entry);
  }
  private async attach(entry: Entry) {
    const api = this.api;
    if (!api || !this.active || !entry.host || this.entries.get(entry.id) !== entry) return;
    if (entry.attaching) { entry.replayAgain = true; return; }
    entry.attaching = true;
    const token = ++entry.attachToken;
    const full = entry.fullReplay || entry.writing || entry.writes.length > 0 || entry.pending.size > 0 || entry.sequence > entry.consumed;
    entry.fullReplay = false;
    // ACK/parser callbacks belong to a delivery view, not just a PTY generation.
    // Fence them before discarding a same-generation view, then wait for any
    // already-invoked ACK before detaching so it cannot credit the new view.
    entry.epoch++;
    if (full) {
      this.resetSurface(entry, 0);
      this.update(entry, { phase: 'connecting', warning: 'Reloading the retained terminal buffer.' });
    }
    if (entry.state.generation) this.detach(entry);
    const detached = await entry.detachQueue;
    if (!this.active || entry.attachToken !== token || this.entries.get(entry.id) !== entry) return;
    if (!detached || !entry.host) { entry.attaching = false; return; }
    const result = await request(() => api.attachTerminal({ tabId: entry.id,
      ...(entry.state.generation && !full ? { generation: entry.state.generation, afterSequence: entry.consumed } : {}),
    }));
    if (!this.active || entry.attachToken !== token || this.entries.get(entry.id) !== entry) return;
    if (!result.ok) { entry.attaching = false; this.update(entry, { phase: 'unavailable', error: result.error }); entry.early = []; entry.earlyBytes = 0; entry.replayAgain = false; return; }
    const attachment: TerminalAttachmentDto = result.value;
    const replaced = entry.state.generation !== attachment.generation;
    if (replaced || attachment.truncated || full) {
      if (replaced && entry.state.generation) {
        entry.retired.add(entry.state.generation);
        if (entry.retired.size > 64) entry.retired.delete(entry.retired.values().next().value!);
      }
      entry.epoch++;
      this.resetSurface(entry, attachment.firstSequence - 1);
      // Cursor-addressed output (and a snapshot above all) only reproduces the app's layout when it
      // is parsed at the grid the app was drawing for, not a fresh terminal's 80x24. The following
      // fit resizes to the real viewport.
      entry.surface.resize?.(attachment.cols, attachment.rows);
      // A raw tail of a diff-rendering TUI lacks every row it did not rewrite recently; the only
      // way to complete the screen is to make the app repaint itself.
      entry.repaint = !attachment.snapshot && attachment.truncated && attachment.state === 'open';
    }
    this.update(entry, { generation: attachment.generation, phase: attachment.state, exitCode: attachment.exitCode, error: null,
      warning: attachment.snapshot ? null : attachment.truncated || full ? 'Earlier output is unavailable. Showing the retained terminal buffer.' : entry.state.warning });
    for (const chunk of attachment.chunks) this.apply(entry, chunk);
    const early = entry.early; entry.early = []; entry.earlyBytes = 0;
    // The attachment, not arbitrary event arrival order, selects the generation.
    let generationRefresh = false;
    for (const event of early) {
      if (event.generation === attachment.generation) this.apply(entry, event);
      else if (!entry.retired.has(event.generation)) generationRefresh = true;
    }
    entry.attaching = false;
    if (!entry.host) { this.detach(entry); return; }
    if (entry.pending.size && attachment.lastSequence >= Math.min(...entry.pending.keys())) {
      entry.replayAgain = false; entry.fullReplay = true;
      this.update(entry, { phase: 'unavailable', error: { code: 'INTERNAL', message: 'Terminal replay has a sequence gap. Refresh to load the retained buffer; no shell was restarted.', retryable: true } });
      return;
    }
    this.fit(entry.id); void this.acknowledge(entry);
    if (entry.visible) this.focus(entry.id);
    const again = generationRefresh || (entry.replayAgain && entry.pending.size > 0); entry.replayAgain = false;
    if (again || entry.fullReplay) void this.attach(entry);
  }
  private resetSurface(entry: Entry, sequence: number) {
    entry.surface.dispose();
    const inputEpoch = ++entry.inputEpoch;
    entry.surface = this.factory(data => { if (entry.inputEpoch === inputEpoch) this.input(entry, data); }, this.theme, this.clipboard(() => entry));
    if (entry.host) entry.host.append(entry.surface.element);
    entry.writes = []; entry.writeBytes = 0; entry.writing = false;
    entry.pending.clear(); entry.pendingBytes = 0;
    entry.sequence = sequence; entry.consumed = sequence; entry.nextAck = 0; entry.resized = '';
  }
  private write(entry: Entry, data: string, sequence: number): boolean {
    const bytes = encoder.encode(data).length;
    if (entry.writeBytes + bytes > MAX_BUFFER) {
      entry.fullReplay = true;
      this.update(entry, { warning: 'Output exceeded the display queue. Reloading the retained buffer.' });
      void this.attach(entry); return false;
    }
    // Parse replay incrementally in bounded batches. Thousands of tiny valid
    // chunks do not need thousands of parser callbacks or a second object cap.
    const tail = entry.writes.at(-1);
    if (tail && tail.sequence + 1 === sequence && tail.bytes + bytes <= WRITE_BATCH_BYTES) {
      tail.data += data; tail.bytes += bytes; tail.sequence = sequence;
    } else entry.writes.push({ data, sequence, bytes });
    entry.writeBytes += bytes;
    this.drain(entry);
    return true;
  }
  private drain(entry: Entry) {
    if (entry.writing || !entry.writes.length) return;
    entry.writing = true;
    const { data, sequence, bytes } = entry.writes.shift()!; const epoch = entry.epoch;
    entry.surface.write(data, () => {
      if (epoch !== entry.epoch || this.entries.get(entry.id) !== entry) return;
      entry.writeBytes -= bytes; entry.writing = false;
      entry.consumed = sequence; entry.nextAck = Math.max(entry.nextAck, sequence);
      void this.acknowledge(entry); this.drain(entry);
    });
  }
  private detach(entry: Entry) {
    const api = this.api; const generation = entry.state.generation;
    if (!api || !generation) return;
    // Serialize detach before a rapid reattachment so a late detach cannot
    // switch off the newly visible view for the same runtime generation.
    const ackTask = entry.ackTask;
    entry.detachQueue = entry.detachQueue.then(async () => {
      await ackTask;
      const result = await request(() => api.detachTerminal({ tabId: entry.id, generation }));
      if (!result.ok && this.active && this.entries.get(entry.id) === entry) this.update(entry, { phase: 'unavailable', error: result.error });
      return result.ok;
    });
  }
  private acknowledge(entry: Entry) {
    const api = this.api;
    if (!api || entry.ackTask || entry.attaching || !entry.host) return;
    const epoch = entry.epoch;
    // Defer invocation until ackTask is assigned. A synchronous backend pump
    // may request recovery inside acknowledgeTerminal itself.
    const task = Promise.resolve().then(async () => {
      while (this.active && entry.host && !entry.attaching && epoch === entry.epoch && entry.nextAck > 0 && entry.state.generation) {
        const generation = entry.state.generation; const sequence = entry.nextAck; entry.nextAck = 0;
        const result = await request(() => api.acknowledgeTerminal({ tabId: entry.id, generation, sequence }));
        if (!this.active || this.entries.get(entry.id) !== entry || epoch !== entry.epoch) return;
        if (entry.state.generation !== generation) return;
        if (!result.ok) {
          entry.fullReplay = true;
          this.update(entry, { error: result.error });
          void this.attach(entry);
          return;
        }
      }
    }).finally(() => {
      if (entry.ackTask !== task) return;
      entry.ackTask = null;
      if (this.active && !entry.attaching && entry.nextAck > 0 && epoch === entry.epoch) void this.acknowledge(entry);
    });
    entry.ackTask = task;
    return task;
  }
  private input(entry: Entry, data: string) {
    const generation = entry.state.generation; const api = this.api;
    if (!api || (!entry.visible && !entry.writing) || entry.state.phase !== 'open' || !generation || !data || data.includes('\0')) return;
    const bytes = encoder.encode(data).length;
    if (bytes > 65536 || entry.inputBytes + bytes > 65536) {
      this.update(entry, { error: { code: 'VALIDATION', message: 'Terminal input is too large or arriving too quickly. Paste smaller sections.', retryable: false } }); return;
    }
    entry.inputBytes += bytes;
    entry.inputQueue = entry.inputQueue.then(async () => {
      if (!this.active || entry.state.generation !== generation || entry.state.phase !== 'open') return;
      const result = await request(() => api.writeTerminal({ tabId: entry.id, generation, data }));
      if (!result.ok && this.active && this.entries.get(entry.id) === entry && entry.state.generation === generation) this.update(entry, { error: result.error });
    }).finally(() => { entry.inputBytes -= bytes; });
  }
  private async resize(entry: Entry) {
    const api = this.api;
    if (!api || entry.resizing) return;
    entry.resizing = true;
    try {
      while (entry.nextSize && this.active && entry.visible && entry.state.phase === 'open' && entry.state.generation) {
        const size = entry.nextSize; entry.nextSize = null; const generation = entry.state.generation;
        const key = `${generation}:${size.cols}:${size.rows}`;
        if (entry.repaint) {
          // SIGWINCH is only delivered, and TUIs (Node's tty 'resize', ncurses) only repaint, when the
          // size really changes: shrink by one row, give the app time to notice, then restore.
          entry.repaint = false; entry.resized = '';
          const nudged = await request(() => api.resizeTerminal({ tabId: entry.id, generation, cols: size.cols, rows: Math.max(2, size.rows - 1) }));
          if (!this.active || this.entries.get(entry.id) !== entry) break;
          if (nudged.ok) await new Promise(resolve => setTimeout(resolve, REPAINT_NUDGE_MS));
          if (!this.active || this.entries.get(entry.id) !== entry || entry.state.generation !== generation) continue;
        }
        if (entry.resized === key) continue;
        const result = await request(() => api.resizeTerminal({ tabId: entry.id, generation, ...size }));
        if (!this.active || this.entries.get(entry.id) !== entry) break;
        if (entry.state.generation !== generation) continue;
        if (!result.ok) { this.update(entry, { error: result.error }); break; }
        entry.resized = key;
      }
    } finally { entry.resizing = false; }
  }
}
