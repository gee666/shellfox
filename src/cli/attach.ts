import type { WriteStream } from 'node:tty';
import type { EventEmitter } from 'node:events';
import { StringDecoder } from 'node:string_decoder';
import type { OwnedTerminalAttachmentDto, Result, TerminalApi, TerminalEvent } from '../shared/contracts';
import { CliFailure, unwrap } from './output';
import { ENTER_TERMINAL_SCREEN, LEAVE_TERMINAL_SCREEN, RESET_TERMINAL_MODES, terminalGrid } from './terminal-presentation';
import { TerminalQueryFilter } from './terminal-queries';
export type AttachApi = Pick<TerminalApi, 'attachTerminal' | 'takeTerminalControl' | 'writeTerminal' | 'resizeTerminal' | 'acknowledgeTerminal' | 'detachTerminal' | 'subscribeTerminal'>;
type Events = Pick<EventEmitter, 'on' | 'off'>;
export interface AttachIo {
  input: Events & { isTTY: boolean; isRaw: boolean; setRawMode(value: boolean): unknown; resume(): unknown; pause(): unknown };
  output: Events & Pick<WriteStream, 'write' | 'isTTY' | 'columns' | 'rows'>;
  signals: Events;
  onDisconnect?: (listener: () => void) => () => void;
}
const clamp = (value: number | undefined, fallback: number) => Math.max(2, Math.min(500, value || fallback));
/** Ownership changes never retry input. A lost reply may already have written it. */
export async function attachTerminal(api: AttachApi, tabId: string, io: AttachIo): Promise<void> {
  if (!io.input.isTTY || !io.output.isTTY) throw new CliFailure('VALIDATION', 'Interactive attach requires a TTY on stdin and stdout.');
  const initialCols = clamp(io.output.columns, 80), initialRows = clamp(io.output.rows, 24);
  const wasRaw = io.input.isRaw;
  const decoder = new StringDecoder('utf8'), queries = new TerminalQueryFilter();
  let generation: string | undefined, controlEpoch = 0, sequence = 0;
  let stopped = false, attached = false, owned = false, taking = false;
  let observedControl: Extract<TerminalEvent, { type: 'control' }> | undefined;
  let presentationStarted = false, gridRequested = false, replaying = false;
  let clientCols = initialCols, clientRows = initialRows, pendingInputBytes = 0;
  let queuedBytes = 0, events: TerminalEvent[] = [], processing: Promise<void> = Promise.resolve();
  let pendingSize: { cols: number; rows: number; controlEpoch: number } | null = null, resizing = false;
  let resolveDone: () => void = () => {}, rejectDone: (error: unknown) => void = () => {};
  const done = new Promise<void>((resolve, reject) => { resolveDone = resolve; rejectDone = reject; });
  void done.catch(() => {});
  const finish = (error?: unknown) => {
    if (stopped) return;
    stopped = true;
    try { io.input.setRawMode(Boolean(wasRaw)); io.input.pause(); } catch { /* The caller's terminal may have closed. */ }
    if (error) rejectDone(error); else resolveDone();
  };
  const enqueue = (action: () => Promise<void>) => {
    processing = processing.then(async () => { if (!stopped) await action(); }).catch(error => { finish(error); });
  };
  const write = (data: string): Promise<void> => new Promise((resolve, reject) => {
    io.output.write(data, error => error ? reject(error) : resolve());
  });
  const showBlocked = async () => {
    if (stopped) return;
    owned = false; pendingSize = null; queries.reset();
    await write(RESET_TERMINAL_MODES + '\x1b[2J\x1b[HAnother client is controlling this terminal.\r\n\r\nCtrl+T: Take it    Ctrl+]: Detach\r\n');
  };
  const controlledRequest = async <T>(work: Promise<Result<T>>): Promise<boolean> => {
    const result = await work;
    if (stopped) return false;
    if (!result.ok && result.error.code === 'TERMINAL_CONTROL_REQUIRED') { await showBlocked(); return false; }
    unwrap(result); return true;
  };
  const acknowledge = async () => {
    if (owned && generation && sequence > 0) await controlledRequest(api.acknowledgeTerminal({ tabId, generation, controlEpoch, sequence }));
  };
  const replayCurrent = (snapshot: OwnedTerminalAttachmentDto) => !stopped && snapshot.controlEpoch >= controlEpoch
    && (!observedControl || observedControl.generation !== snapshot.generation
      || (observedControl.controlEpoch <= snapshot.controlEpoch && !(observedControl.controlEpoch === snapshot.controlEpoch && observedControl.control === 'blocked')));
  const present = async (snapshot: OwnedTerminalAttachmentDto) => {
    if (stopped || snapshot.controlEpoch < controlEpoch) return;
    generation = snapshot.generation;
    if (!replayCurrent(snapshot)) { await showBlocked(); return; }
    controlEpoch = snapshot.controlEpoch; owned = true; queries.reset();
    let restored = false;
    replaying = true;
    try {
      await write(RESET_TERMINAL_MODES + '\x1b[2J\x1b[H');
      if (!replayCurrent(snapshot)) return;
      gridRequested = true;
      await write(terminalGrid(snapshot.cols, snapshot.rows));
      for (const chunk of snapshot.chunks) {
        if (!replayCurrent(snapshot)) return;
        const data = queries.push(chunk.data, true);
        if (data) await write(data);
      }
      if (!replayCurrent(snapshot)) return;
      sequence = snapshot.lastSequence;
      await acknowledge();
      if (!owned || stopped) return;
      await write(terminalGrid(clientCols, clientRows));
      restored = true;
      if (!owned || !replayCurrent(snapshot)) return;
      if (snapshot.state === 'closed') { finish(); return; }
      if (snapshot.cols === clientCols && snapshot.rows === clientRows) {
        // Equal dimensions do not emit SIGWINCH. Repaint a restored TUI once,
        // just as a small manual resize would, without an ongoing size heartbeat.
        await controlledRequest(api.resizeTerminal({ tabId, generation, controlEpoch, cols: clientCols > 2 ? clientCols - 1 : 3, rows: clientRows }));
        // ConPTY can coalesce immediate resize pairs before the program sees them.
        await new Promise<void>(resolve => setTimeout(resolve, 100));
        if (!owned || !replayCurrent(snapshot)) return;
      }
      await controlledRequest(api.resizeTerminal({ tabId, generation, controlEpoch, cols: clientCols, rows: clientRows }));
    } finally {
      if (!stopped && !restored) await write(terminalGrid(clientCols, clientRows));
      replaying = false;
    }
  };
  const refresh = async () => {
    if (stopped) return;
    const snapshot = unwrap(await api.attachTerminal({ tabId, ...(generation ? { generation } : {}) }));
    if (stopped) { await api.detachTerminal({ tabId, generation: snapshot.generation }).catch(() => {}); return; }
    if (snapshot.controlEpoch < controlEpoch) return;
    generation = snapshot.generation; controlEpoch = snapshot.controlEpoch;
    if (snapshot.control === 'blocked') await showBlocked(); else await present(snapshot);
  };
  const consume = async (event: TerminalEvent) => {
    if (stopped || event.type === 'activity' || event.generation !== generation) return;
    if (event.type === 'control') {
      if (event.controlEpoch < controlEpoch) return;
      if (event.controlEpoch === controlEpoch && ((event.control === 'owned' && owned) || (event.control === 'blocked' && !owned))) return;
      controlEpoch = event.controlEpoch;
      if (event.control === 'blocked') await showBlocked();
      else { owned = false; await refresh(); }
      return;
    }
    if (event.type === 'exit' && !owned && event.controlEpoch >= controlEpoch) { finish(); return; }
    if (event.controlEpoch !== controlEpoch) return;
    if (event.type === 'exit') {
      if (owned && event.lastSequence > sequence) throw new CliFailure('TARGET_LOST', 'Terminal output ended with missing data. Reattach to replay the screen.');
      finish(); return;
    }
    if (!owned) return;
    if (event.type === 'error') throw new CliFailure(event.error.code, event.error.message, event.error.retryable);
    if (event.sequence <= sequence) return;
    if (event.sequence !== sequence + 1) throw new CliFailure('TARGET_LOST', 'Terminal output was interrupted. Reattach to replay the screen.');
    const data = queries.push(event.data, false);
    if (data) await write(data);
    sequence = event.sequence; await acknowledge();
  };
  const queueEvent = (event: TerminalEvent) => {
    const size = event.type === 'data' ? Buffer.byteLength(event.data) : 0;
    enqueue(async () => { try { await consume(event); } finally { queuedBytes -= size; } });
  };
  const unsubscribe = api.subscribeTerminal(event => {
    if (event.tabId !== tabId || stopped) return;
    if (event.type === 'control' && (!generation || event.generation === generation)
      && (!observedControl || observedControl.generation !== event.generation || event.controlEpoch >= observedControl.controlEpoch)) {
      observedControl = event;
      if (event.control === 'blocked' || event.controlEpoch > controlEpoch) {
        owned = false; pendingSize = null;
        if (event.control === 'blocked' && presentationStarted) void showBlocked().catch(error => finish(error));
      }
    }
    queuedBytes += event.type === 'data' ? Buffer.byteLength(event.data) : 0;
    if (queuedBytes > 1024 * 1024) { finish(new CliFailure('TARGET_LOST', 'Terminal output exceeded the attachment buffer. Reattach to replay the screen.')); return; }
    if (!attached) events.push(event); else queueEvent(event);
  });
  const input = (chunk: Buffer | string) => {
    if (stopped) return;
    const text = typeof chunk === 'string' ? chunk : decoder.write(chunk);
    const detach = text.indexOf('\x1d');
    if (!generation) { if (detach >= 0) finish(); return; }
    if (!owned) {
      if (detach >= 0) { finish(); return; }
      if (text.includes('\x14') && !taking) {
        taking = true;
        enqueue(async () => {
          try { if (!owned && generation) unwrap(await api.takeTerminalControl({ tabId, generation })); }
          finally { taking = false; }
        });
      }
      return;
    }
    const data = detach < 0 ? text : text.slice(0, detach);
    if (data) {
      const bytes = Buffer.byteLength(data);
      if (bytes > 65536 || pendingInputBytes + bytes > 131072) {
        finish(new CliFailure('VALIDATION', 'Terminal input exceeds the 64 KiB frame or 128 KiB pending-input limit. Some earlier input may already have been delivered; nothing was retried.'));
        return;
      }
      pendingInputBytes += bytes;
      const currentGeneration = generation, epoch = controlEpoch;
      enqueue(async () => {
        try {
          if (owned && controlEpoch === epoch) {
            try { await controlledRequest(api.writeTerminal({ tabId, generation: currentGeneration, controlEpoch: epoch, data })); }
            catch (error) {
              if (error instanceof CliFailure) throw error;
              throw new CliFailure('TARGET_LOST', 'Terminal input delivery is uncertain; input was not retried. ' + (error instanceof Error ? error.message : 'Host request failed.'));
            }
          }
        } finally { pendingInputBytes -= bytes; }
      });
    }
    if (detach >= 0) {
      io.input.pause();
      if (pendingInputBytes === 0) finish(); else enqueue(async () => { finish(); });
    }
  };
  const resize = () => {
    if (replaying) return;
    clientCols = clamp(io.output.columns, initialCols); clientRows = clamp(io.output.rows, initialRows);
    if (!generation || stopped || !owned) return;
    pendingSize = { cols: clientCols, rows: clientRows, controlEpoch };
    if (resizing) return;
    resizing = true;
    enqueue(async () => {
      try {
        while (pendingSize && owned && generation) {
          const size = pendingSize; pendingSize = null;
          if (size.controlEpoch === controlEpoch) await controlledRequest(api.resizeTerminal({ tabId, generation, ...size }));
        }
      } finally { resizing = false; }
    });
  };
  const end = () => { enqueue(async () => { finish(); }); };
  const failed = (error: Error) => { finish(error); };
  const signal = () => { finish(); };
  const disconnected = io.onDisconnect?.(() => finish(new CliFailure('TARGET_LOST', 'Host connection closed. Input delivery may be uncertain; input was not retried.')));
  io.input.on('error', failed); io.input.on('end', end); io.output.on('error', failed);
  const signalNames: NodeJS.Signals[] = process.platform === 'win32' ? ['SIGINT', 'SIGTERM', 'SIGBREAK'] : ['SIGINT', 'SIGTERM', 'SIGHUP'];
  for (const name of signalNames) io.signals.on(name, signal);
  try {
    presentationStarted = true;
    await write(ENTER_TERMINAL_SCREEN);
    io.input.setRawMode(true);
    io.input.on('data', input);
    io.input.resume();
    // Read keys during setup too. Writes wait behind replay; detach must not be
    // swallowed by a cooked input buffer just before raw mode is enabled.
    const initializing = refresh();
    processing = initializing.catch(error => { finish(error); });
    await Promise.race([initializing, done]);
    if (!stopped) io.output.on('resize', resize);
    attached = true;
    for (const event of events) queueEvent(event);
    events = [];
    await done;
  } finally {
    finish(); unsubscribe(); disconnected?.();
    io.input.off('data', input); io.input.off('end', end); io.input.off('error', failed);
    io.output.off('resize', resize);
    for (const name of signalNames) io.signals.off(name, signal);
    if (presentationStarted) await write(RESET_TERMINAL_MODES + (gridRequested ? terminalGrid(clientCols, clientRows) : '') + LEAVE_TERMINAL_SCREEN).catch(() => {});
    io.output.off('error', failed);
    if (generation) await api.detachTerminal({ tabId, generation }).catch(() => {});
  }
}
