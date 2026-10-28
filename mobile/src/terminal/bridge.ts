import type { Result, TerminalApi, TerminalDataEvent, TerminalEvent } from '../../../src/shared/contracts';
import { responseSchemas, terminalEventSchema } from '../../../src/shared/schemas';
import { deliveryOperation, detachAttachment, trackAttachment } from './delivery-queue';
import { TerminalOperationError, unwrapTerminalResult as unwrap } from './errors';
import { emptyModifiers, terminalFontSize, terminalKeyEventSchema, terminalModifiersSchema, utf8Size, type HostMessage, type PageMessage, type TerminalKeyEvent, type TerminalModifiers, type TerminalThemeInputs } from './protocol';

export type MobileTerminalApi = Pick<TerminalApi, 'attachTerminal' | 'writeTerminal' | 'resizeTerminal' | 'acknowledgeTerminal' | 'detachTerminal' | 'takeTerminalControl'>;
export type SubscribeTerminal = (listener: (event: TerminalEvent) => void) => () => void;
export type TerminalBridgeState = 'attaching' | 'attached' | 'closed' | 'reconnecting' | 'blocked';
interface Options {
  api: MobileTerminalApi; subscribeTerminal: SubscribeTerminal; tabId: string; viewId: string; fontSize: number;
  send: (message: HostMessage) => void; onError: (error: Error) => void;
  onSelection?: (text: string) => void;
  onFocus?: () => void;
  onState?: (state: TerminalBridgeState) => void;
  theme?: TerminalThemeInputs;
}
let nextEpoch = 0;

/** Owns one delivery view. Only a matching xterm render callback can release output credit. */
export class TerminalBridge {
  private alive = true;
  private state: TerminalBridgeState | null = null;
  private epoch = '';
  private generation: string | null = null;
  private retired = new Set<string>();
  private attaching = false;
  private attached = false;
  private open = false;
  private received = 0;
  private consumed = 0;
  private pendingAck: { epoch: string; sequence: number } | null = null;
  private ticket = 0;
  private pending: { ticket: number; sequence: number } | null = null;
  private queue: TerminalDataEvent[] = [];
  private queuedBytes = 0;
  private early: TerminalEvent[] = [];
  private earlyBytes = 0;
  private earlyOverflow = false;
  private inputQueue: Promise<void> = Promise.resolve();
  private inputBytes = 0;
  private pendingInput: { data: string; bytes: number; epoch: string } | null = null;
  private modifiers: TerminalModifiers = { ...emptyModifiers };
  private nextSize: { cols: number; rows: number } | null = null;
  private resizing = false;
  private repaint = false;
  private controlEpoch = 0;
  private refreshControl = false;
  private taking = false;
  private selecting = false;
  private focusPending = false;
  private fontSize: number;
  private recovery: ReturnType<typeof setTimeout> | null = null;
  private unsubscribe: (() => void) | null = null;

  constructor(private readonly options: Options) { this.fontSize = terminalFontSize(options.fontSize); }
  getUploadIdentity(): { tabId: string; generation: string; controlEpoch: number } | null {
    return this.alive && this.attached && this.open && this.generation
      ? { tabId: this.options.tabId, generation: this.generation, controlEpoch: this.controlEpoch } : null;
  }
  start(): void {
    this.unsubscribe = this.options.subscribeTerminal(event => this.event(event));
    void this.attach();
  }
  private send(message: HostMessage): void { if (this.alive) this.options.send(message); }
  private identity() { return { viewId: this.options.viewId, epoch: this.epoch }; }
  private report(error: unknown): void {
    if (this.state === 'blocked' && error instanceof TerminalOperationError && error.detail.code === 'TERMINAL_CONTROL_REQUIRED') return;
    if (this.alive) this.options.onError(error instanceof Error ? error : new Error(String(error)));
  }
  private setState(state: TerminalBridgeState): void {
    if (!this.alive || this.state === state) return;
    this.state = state; this.options.onState?.(state);
  }
  private controlResult<T>(result: Result<T>, epoch: string): T {
    if (!result.ok && epoch === this.epoch) {
      if (result.error.code === 'TARGET_LOST') this.recover();
      if (result.error.code === 'TERMINAL_CONTROL_REQUIRED') this.block(this.controlEpoch);
    }
    return unwrap(result);
  }
  setTheme(theme: TerminalThemeInputs): void {
    this.options.theme = theme;
    if (this.attached) this.send({ type: 'theme', ...this.identity(), ...theme });
  }
  setFontSize(size: number): void {
    this.fontSize = terminalFontSize(size);
    if (this.attached) this.send({ type: 'font', ...this.identity(), fontSize: this.fontSize });
  }
  focus(focused: boolean): void {
    this.focusPending = focused && this.attached;
    if (this.attached) this.send({ type: 'focus', ...this.identity(), focused });
  }
  select(enabled: boolean): void {
    this.selecting = enabled;
    if (this.attached) this.send({ type: 'select', ...this.identity(), enabled });
  }
  paste(data: string): void {
    if (utf8Size(data) > 65000 || data.includes('\0')) { this.report(new Error('Paste a smaller section of text.')); return; }
    if (this.attached && this.open && data) this.send({ type: 'paste', ...this.identity(), data });
  }
  key(event: TerminalKeyEvent): void {
    const parsed = terminalKeyEventSchema.safeParse(event);
    if (!parsed.success) { this.report(new Error('Invalid terminal key.')); return; }
    if (this.alive && this.attached && this.open) this.send({ type: 'key', ...this.identity(), event: parsed.data });
  }
  setModifiers(modifiers: TerminalModifiers): void {
    const parsed = terminalModifiersSchema.safeParse(modifiers);
    if (!parsed.success) { this.report(new Error('Invalid terminal modifiers.')); return; }
    this.modifiers = parsed.data;
    if (this.attached) this.send({ type: 'modifiers', ...this.identity(), modifiers: this.modifiers });
  }
  receive(message: PageMessage): void {
    if (!this.alive || message.type === 'ready' || message.viewId !== this.options.viewId || message.epoch !== this.epoch || !this.attached) return;
    switch (message.type) {
      case 'input': this.input(message.data); break;
      case 'resize': this.resize({ cols: message.cols, rows: message.rows }); break;
      case 'rendered': {
        if (!this.pending || message.ticket !== this.pending.ticket) return;
        this.consumed = this.pending.sequence; this.pending = null;
        void this.acknowledge(this.consumed); this.drain(); break;
      }
      case 'selection': this.options.onSelection?.(message.text); break;
      case 'focused':
        if (this.focusPending) { this.focusPending = false; this.options.onFocus?.(); }
        break;
      case 'error': this.report(new Error(message.message)); this.recover(); break;
    }
  }
  async takeControl(): Promise<void> {
    if (!this.alive || this.state !== 'blocked' || !this.generation || this.taking) return;
    this.taking = true;
    try {
      unwrap(responseSchemas.takeTerminalControl.parse(await this.options.api.takeTerminalControl({ tabId: this.options.tabId, generation: this.generation })));
      if (this.alive && this.state === 'blocked' && !this.attaching) await this.attach(false);
    } catch (error) { this.report(error); }
    finally { this.taking = false; }
  }
  private block(controlEpoch: number): void {
    this.controlEpoch = controlEpoch; this.attached = false; this.open = false; this.focusPending = false;
    this.epoch = String(++nextEpoch); this.pending = null; this.pendingAck = null; this.pendingInput = null;
    this.queue = []; this.queuedBytes = 0; this.nextSize = null; this.refreshControl = false;
    if (this.recovery) { clearTimeout(this.recovery); this.recovery = null; }
    this.send({ type: 'clear', ...this.identity() }); this.options.onSelection?.(''); this.setState('blocked');
  }
  private async attach(detach = true): Promise<void> {
    if (!this.alive || this.attaching) return;
    this.setState(this.state ? 'reconnecting' : 'attaching');
    this.attaching = true; this.attached = false; this.nextSize = null; this.focusPending = false;
    this.epoch = String(++nextEpoch); this.pending = null;
    this.queue = []; this.queuedBytes = 0; this.early = []; this.earlyBytes = 0; this.earlyOverflow = false;
    const { api, tabId } = this.options;
    try {
      const attachment = await deliveryOperation(api, tabId, async () => {
        if (detach) await detachAttachment(api, tabId);
        if (!this.alive) return null;
        const value = unwrap(responseSchemas.attachTerminal.parse(await api.attachTerminal({ tabId })));
        if (value.tabId !== tabId) throw new Error('Terminal attachment belongs to another tab.');
        trackAttachment(api, tabId, value.generation);
        if (!this.alive) { await detachAttachment(api, tabId); return null; }
        return value;
      });
      if (!this.alive || !attachment) return;
      if (this.generation && this.generation !== attachment.generation) {
        this.retired.add(this.generation); this.controlEpoch = 0;
        if (this.retired.size > 16) this.retired.delete(this.retired.values().next().value!);
      }
      this.generation = attachment.generation;
      if (attachment.controlEpoch < this.controlEpoch) return;
      this.controlEpoch = attachment.controlEpoch; this.refreshControl = false;
      if (attachment.control === 'blocked') { this.block(attachment.controlEpoch); return; }
      if (attachment.chunks.some((chunk, index) => chunk.sequence !== attachment.firstSequence + index)
        || (attachment.chunks.length ? attachment.chunks.at(-1)!.sequence !== attachment.lastSequence : attachment.lastSequence !== attachment.firstSequence - 1)) {
        throw new Error('Terminal replay has a sequence gap.');
      }
      this.received = attachment.lastSequence; this.consumed = 0; this.open = attachment.state === 'open';
      this.repaint = attachment.truncated && !attachment.snapshot && this.open;
      this.send({ type: 'reset', ...this.identity(), cols: attachment.cols, rows: attachment.rows, fontSize: this.fontSize });
      if (this.options.theme) this.send({ type: 'theme', ...this.identity(), ...this.options.theme });
      this.send({ type: 'modifiers', ...this.identity(), modifiers: this.modifiers });
      if (this.selecting) this.send({ type: 'select', ...this.identity(), enabled: true });
      this.options.onSelection?.(''); this.attached = true;
      this.setState(this.open ? 'attached' : 'closed');
      const ticket = ++this.ticket;
      this.pending = { ticket, sequence: attachment.lastSequence };
      this.send({ type: 'output', ...this.identity(), ticket, data: attachment.chunks.map(c => c.data).join(''), historical: true, fit: true });
      const early = this.early; this.early = [];
      this.attaching = false;
      if (this.earlyOverflow) { this.recover(); return; }
      for (const event of early) { if (!this.attached) break; this.apply(event); }
    } catch (error) { this.attached = false; this.report(error); this.recover(); }
    finally {
      this.attaching = false;
      if (this.refreshControl && this.state !== 'blocked') { this.refreshControl = false; void this.attach(false); }
    }
  }
  private event(raw: unknown): void {
    if (!this.alive) return;
    const parsed = terminalEventSchema.safeParse(raw);
    if (!parsed.success) { this.report(new Error('Invalid terminal event.')); this.recover(); return; }
    const event = parsed.data;
    if (event.tabId !== this.options.tabId || event.type === 'activity') return;
    if (event.type === 'control') {
      if ((this.generation && event.generation !== this.generation) || event.controlEpoch < this.controlEpoch) return;
      if (event.control === 'blocked') { this.block(event.controlEpoch); return; }
      if (event.controlEpoch === this.controlEpoch && this.attached) return;
      this.controlEpoch = event.controlEpoch;
      if (this.attaching) { this.refreshControl = true; this.setState('attaching'); }
      else void this.attach(false);
      return;
    }
    if (this.attaching) {
      const bytes = event.type === 'data' ? utf8Size(event.data) : 0;
      if (this.early.length >= 1024 || this.earlyBytes + bytes > 262144) { this.earlyOverflow = true; return; }
      this.early.push(event); this.earlyBytes += bytes; return;
    }
    if (this.attached) this.apply(event);
  }
  private apply(event: TerminalEvent): void {
    if (event.type === 'activity' || event.type === 'control') return;
    if (event.controlEpoch !== this.controlEpoch) return;
    if (event.generation !== this.generation) { if (!this.retired.has(event.generation)) this.recover(); return; }
    if (event.type === 'error') { this.report(new TerminalOperationError(event.error)); this.recover(); return; }
    if (event.type === 'exit') {
      this.open = false; this.setState('closed');
      if (event.lastSequence > this.received) this.recover();
      return;
    }
    if (event.sequence <= this.received) return;
    const bytes = utf8Size(event.data);
    if (event.sequence !== this.received + 1 || this.queuedBytes + bytes > 262144 || this.queue.length >= 8192) { this.recover(); return; }
    this.received = event.sequence; this.queue.push(event); this.queuedBytes += bytes; this.drain();
  }
  private drain(): void {
    if (!this.attached || this.pending || !this.queue.length) return;
    let data = '', sequence = 0, bytes = 0;
    while (this.queue.length && bytes < 32768) {
      const event = this.queue.shift()!;
      data += event.data; sequence = event.sequence; bytes += utf8Size(event.data);
    }
    this.queuedBytes -= bytes;
    const ticket = ++this.ticket; this.pending = { ticket, sequence };
    this.send({ type: 'output', ...this.identity(), ticket, data, historical: false, fit: false });
  }
  private async acknowledge(sequence: number): Promise<void> {
    const epoch = this.epoch, generation = this.generation, controlEpoch = this.controlEpoch;
    if (!this.alive || !this.attached || !generation) return;
    if (this.pendingAck?.epoch === epoch) { this.pendingAck.sequence = Math.max(this.pendingAck.sequence, sequence); return; }
    const receipt = { epoch, sequence }; this.pendingAck = receipt;
    try {
      await deliveryOperation(this.options.api, this.options.tabId, async () => {
        if (this.pendingAck === receipt) this.pendingAck = null;
        if (!this.alive || !this.attached || epoch !== this.epoch) return;
        this.controlResult(responseSchemas.acknowledgeTerminal.parse(await this.options.api.acknowledgeTerminal({ tabId: this.options.tabId, generation, controlEpoch, sequence: receipt.sequence })), epoch);
      });
    } catch (error) { if (epoch === this.epoch) { this.report(error); this.recover(); } }
  }
  private recover(): void {
    if (!this.alive || this.recovery) return;
    this.attached = false; this.send({ type: 'clear', ...this.identity() }); this.setState('reconnecting');
    this.recovery = setTimeout(() => { this.recovery = null; void this.attach(); }, 250);
  }
  input(data: string): void {
    const generation = this.generation, epoch = this.epoch, controlEpoch = this.controlEpoch;
    if (!this.alive || !this.attached || !this.open || !generation || !data || data.includes('\0')) return;
    const bytes = utf8Size(data);
    if (this.inputBytes + bytes > 65536) { this.report(new Error('Paste a smaller section of text.')); return; }
    this.inputBytes += bytes;
    if (this.pendingInput?.epoch === epoch) {
      this.pendingInput.data += data; this.pendingInput.bytes += bytes; return;
    }
    // Combine only adjacent writes not yet dispatched. No timer, replay or extra input.
    const batch = { data, bytes, epoch }; this.pendingInput = batch;
    this.inputQueue = this.inputQueue.then(async () => {
      if (this.pendingInput === batch) this.pendingInput = null;
      if (!this.alive || !this.attached || epoch !== this.epoch) return;
      const result = responseSchemas.writeTerminal.parse(await this.options.api.writeTerminal({ tabId: this.options.tabId, generation, controlEpoch, data: batch.data }));
      this.controlResult(result, epoch);
    }).catch(error => {
      if (epoch === this.epoch) this.report(error);
    }).finally(() => { this.inputBytes -= batch.bytes; });
  }
  private resize(size: { cols: number; rows: number }): void {
    const generation = this.generation, epoch = this.epoch, controlEpoch = this.controlEpoch;
    if (!this.alive || !this.attached || !this.open || !generation) return;
    this.nextSize = size;
    if (this.resizing) return;
    this.resizing = true; this.pendingInput = null;
    // Coalesce keyboard animation sizes and preserve input ordering.
    this.inputQueue = this.inputQueue.then(async () => {
      if (!this.alive || !this.attached || epoch !== this.epoch || !this.nextSize) return;
      const target = this.nextSize; this.nextSize = null;
      const resize = async (rows: number) => this.controlResult(responseSchemas.resizeTerminal.parse(await this.options.api.resizeTerminal({ tabId: this.options.tabId, generation, controlEpoch, cols: target.cols, rows })), epoch);
      if (this.repaint) {
        // A raw truncated TUI tail needs a real grid change to make the shell repaint missing rows.
        this.repaint = false;
        await resize(target.rows > 2 ? target.rows - 1 : 3);
        await new Promise(resolve => setTimeout(resolve, 60));
        if (!this.alive || !this.attached || epoch !== this.epoch) return;
      }
      await resize(target.rows);
    }).catch(error => { if (epoch === this.epoch) this.report(error); }).finally(() => {
      this.resizing = false;
      if (this.nextSize) this.resize(this.nextSize);
    });
  }
  dispose(): void {
    if (!this.alive) return;
    this.send({ type: 'clear', ...this.identity() });
    this.alive = false; this.attached = false; this.unsubscribe?.();
    if (this.recovery) clearTimeout(this.recovery);
    void deliveryOperation(this.options.api, this.options.tabId, () =>
      detachAttachment(this.options.api, this.options.tabId)).catch(() => undefined);
  }
}
