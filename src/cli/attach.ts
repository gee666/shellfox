import type { WriteStream } from 'node:tty';
import type { EventEmitter } from 'node:events';
import { StringDecoder } from 'node:string_decoder';
import type { TerminalApi, TerminalEvent } from '../shared/contracts';
import { CliFailure, unwrap } from './output';
import { ENTER_TERMINAL_SCREEN, LEAVE_TERMINAL_SCREEN, RESET_TERMINAL_MODES, terminalGrid } from './terminal-presentation';
export type AttachApi = Pick<TerminalApi, 'attachTerminal' | 'writeTerminal' | 'resizeTerminal' | 'acknowledgeTerminal' | 'detachTerminal' | 'subscribeTerminal'>;
type Events = Pick<EventEmitter, 'on' | 'off'>;
export interface AttachIo {
  input: Events & { isTTY: boolean; isRaw: boolean; setRawMode(value: boolean): unknown; resume(): unknown; pause(): unknown };
  output: Events & Pick<WriteStream, 'write' | 'isTTY' | 'columns' | 'rows'>;
  signals: Events;
  onDisconnect?: (listener: () => void) => () => void;
}
const clamp = (value: number | undefined, fallback: number) => Math.max(2, Math.min(500, value || fallback));
/** One attachment, no reconnect or input retry. A lost reply may already have written input. */
export async function attachTerminal(api: AttachApi, tabId: string, io: AttachIo): Promise<void> {
  if (!io.input.isTTY || !io.output.isTTY) throw new CliFailure('VALIDATION', 'Interactive attach requires a TTY on stdin and stdout.');
  const initialCols = clamp(io.output.columns, 80), initialRows = clamp(io.output.rows, 24);
  const wasRaw = io.input.isRaw;
  const decoder = new StringDecoder('utf8');
  let generation: string | undefined, sequence = 0, stopped = false, attached = false;
  let presentationStarted = false, gridRequested = false;
  let clientCols = initialCols, clientRows = initialRows, pendingInputBytes = 0;
  let queuedBytes = 0, events: TerminalEvent[] = [], processing: Promise<void> = Promise.resolve();
  let resolveDone: () => void = () => {}, rejectDone: (error: unknown) => void = () => {};
  const done = new Promise<void>((resolve, reject) => { resolveDone = resolve; rejectDone = reject; });
  // Attaching can fail while events are already arriving.
  void done.catch(() => {});
  const restore = () => { io.input.setRawMode(Boolean(wasRaw)); io.input.pause(); };
  const finish = (error?: unknown) => {
    if (stopped) return;
    stopped = true;
    try { restore(); } catch { /* The terminal may have disappeared. */ }
    if (error) rejectDone(error); else resolveDone();
  };
  const enqueue = (action: () => Promise<void>) => {
    processing = processing.then(action).catch(error => { finish(error); });
  };
  const write = (data: string): Promise<void> => new Promise((resolve, reject) => {
    io.output.write(data, error => error ? reject(error) : resolve());
  });
  const acknowledge = async () => {
    if (generation && sequence > 0) unwrap(await api.acknowledgeTerminal({ tabId, generation, sequence }));
  };
  const consume = async (event: TerminalEvent) => {
    if (stopped || event.type === 'activity' || event.generation !== generation) return;
    if (event.type === 'error') throw new CliFailure(event.error.code, event.error.message, event.error.retryable);
    if (event.type === 'exit') {
      if (event.lastSequence > sequence) throw new CliFailure('TARGET_LOST', 'Terminal output ended with missing data. Reattach to replay the screen.');
      finish(); return;
    }
    if (event.sequence <= sequence) return;
    if (event.sequence !== sequence + 1) throw new CliFailure('TARGET_LOST', 'Terminal output was interrupted. Reattach to replay the screen.');
    await write(event.data); sequence = event.sequence; await acknowledge();
  };
  const unsubscribe = api.subscribeTerminal(event => {
    if (event.tabId !== tabId || stopped) return;
    const size = event.type === 'data' ? Buffer.byteLength(event.data) : 0;
    queuedBytes += size;
    if (queuedBytes > 1024 * 1024) { finish(new CliFailure('TARGET_LOST', 'Terminal output exceeded the attachment buffer. Reattach to replay the screen.')); return; }
    if (!attached) events.push(event);
    else enqueue(async () => { try { await consume(event); } finally { queuedBytes -= size; } });
  });
  const input = (chunk: Buffer | string) => {
    if (stopped || !generation) return;
    const text = typeof chunk === 'string' ? chunk : decoder.write(chunk);
    const detach = text.indexOf('\x1d');
    const data = detach < 0 ? text : text.slice(0, detach);
    if (data) {
      const bytes = Buffer.byteLength(data);
      if (bytes > 65536 || pendingInputBytes + bytes > 131072) {
        finish(new CliFailure('VALIDATION', 'Terminal input exceeds the 64 KiB frame or 128 KiB pending-input limit. Some earlier input may already have been delivered; nothing was retried.'));
        return;
      }
      pendingInputBytes += bytes;
      const currentGeneration = generation;
      enqueue(async () => {
        try {
          if (!stopped) {
            try { unwrap(await api.writeTerminal({ tabId, generation: currentGeneration, data })); }
            catch (error) {
              if (error instanceof CliFailure) throw error;
              throw new CliFailure('TARGET_LOST', 'Terminal input delivery is uncertain; input was not retried. ' + (error instanceof Error ? error.message : 'Host request failed.'));
            }
          }
        } finally { pendingInputBytes -= bytes; }
      });
    }
    if (detach >= 0) { io.input.pause(); enqueue(async () => { finish(); }); }
  };
  const resize = () => {
    if (!generation || stopped) return;
    const currentGeneration = generation;
    const cols = clamp(io.output.columns, initialCols), rows = clamp(io.output.rows, initialRows);
    clientCols = cols; clientRows = rows;
    enqueue(async () => { if (!stopped) unwrap(await api.resizeTerminal({ tabId, generation: currentGeneration, cols, rows })); });
  };
  const end = () => { enqueue(async () => { finish(); }); };
  const failed = (error: Error) => { finish(error); };
  const signal = () => { finish(); };
  const disconnected = io.onDisconnect?.(() => finish(new CliFailure('TARGET_LOST', 'Host connection closed. Input delivery may be uncertain; input was not retried.')));
  io.input.on('error', failed); io.input.on('end', end); io.output.on('error', failed);
  const signalNames: NodeJS.Signals[] = process.platform === 'win32' ? ['SIGINT', 'SIGTERM', 'SIGBREAK'] : ['SIGINT', 'SIGTERM', 'SIGHUP'];
  for (const name of signalNames) io.signals.on(name, signal);
  try {
    const attachment = unwrap(await api.attachTerminal({ tabId }));
    generation = attachment.generation;
    if (stopped) { await done; return; }
    // Replay at the host's grid before asking the PTY to adopt this client's grid.
    // Restore the caller's dimensions after replay, including on terminals honoring CSI 8.
    presentationStarted = true;
    await write(ENTER_TERMINAL_SCREEN);
    gridRequested = true;
    await write(terminalGrid(attachment.cols, attachment.rows));
    for (const chunk of attachment.chunks) await write(chunk.data);
    sequence = attachment.lastSequence;
    await acknowledge();
    await write(terminalGrid(initialCols, initialRows));
    attached = true;
    for (const event of events) enqueue(async () => { await consume(event); });
    events = []; queuedBytes = 0;
    if (attachment.state === 'closed') { await processing; finish(); }
    else {
      unwrap(await api.resizeTerminal({ tabId, generation, cols: initialCols, rows: initialRows }));
      if (!stopped) {
        io.input.setRawMode(true);
        io.input.on('data', input); io.output.on('resize', resize);
        io.input.resume();
      }
    }
    await done;
  } finally {
    finish(); unsubscribe(); disconnected?.();
    io.input.off('data', input); io.input.off('end', end); io.input.off('error', failed);
    io.output.off('resize', resize);
    for (const name of signalNames) io.signals.off(name, signal);
    if (presentationStarted) {
      await write(RESET_TERMINAL_MODES + (gridRequested ? terminalGrid(clientCols, clientRows) : '') + LEAVE_TERMINAL_SCREEN).catch(() => {});
    }
    io.output.off('error', failed);
    // Closing the client is a detach, never a host stop or terminal close.
    if (generation) await api.detachTerminal({ tabId, generation }).catch(() => {});
  }
}
