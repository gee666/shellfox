import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import path from 'node:path';
import { StringDecoder } from 'node:string_decoder';
import { z } from 'zod';
import type { PtyProcess } from './backend';
import { execute } from './profiles';

export { ADMIN_POWERSHELL_PROFILE } from '../../shared/contracts';
const MAX_FRAME = 262144;
const MAX_INPUT = 131072;
const messageSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('ready'), pid: z.number().int().positive(), birth: z.string().regex(/^\d{1,20}$/) }).strict(),
  z.object({ type: z.literal('data'), data: z.string().max(5464).regex(/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/) }).strict(),
  z.object({ type: z.literal('ack'), id: z.number().int().positive().max(Number.MAX_SAFE_INTEGER) }).strict(),
  z.object({ type: z.literal('exit'), exitCode: z.number().int() }).strict(),
  z.object({ type: z.literal('error'), code: z.enum(['cancelled', 'failed']), message: z.string().max(1000) }).strict(),
]);
export interface ElevatedLaunch { cwd: string; env: Record<string, string>; cols: number; rows: number }
export interface ElevatedPty extends PtyProcess {
  readonly birth: string | undefined;
  /** Resolves only after authenticated zero-member job cleanup or proof no shell was started. */
  closeConfirmed(): Promise<void>;
}
export function elevatedHelperPath(options: { resourcesPath?: string; packaged?: boolean; arch?: string } = {}): string {
  const runtime = process as NodeJS.Process & { resourcesPath?: string; defaultApp?: boolean };
  const resources = options.resourcesPath ?? runtime.resourcesPath;
  const packaged = options.packaged ?? (!!resources && runtime.defaultApp !== true);
  if (packaged) {
    if (!resources) throw new Error('Packaged administrator terminals require process.resourcesPath.');
    return path.join(resources, 'elevated-terminal', 'Shellfox.ElevatedTerminal.exe');
  }
  return path.resolve('tmp/terminal-native', `win-${options.arch ?? process.arch}`, 'Shellfox.ElevatedTerminal.exe');
}
export async function elevatedCapability(file = elevatedHelperPath()): Promise<{ available: boolean; reason: string | null }> {
  try {
    const result = await execute(file, ['probe']);
    if (result.toString('utf8').trim() !== 'shellfox-elevated-conpty-v1') throw new Error('Bad helper version.');
    return { available: true, reason: null };
  } catch { return { available: false, reason: 'The bundled administrator ConPTY helper is unavailable. Requires Windows 10 1809 or newer.' }; }
}

/** The unelevated launcher owns a private, inherited stdio channel to this host.
 * It authenticates the elevated pipe peer against ShellExecute's process handle.
 * There is no named-pipe endpoint, token or executable choice in renderer IPC. */
export class WindowsElevatedPty implements ElevatedPty {
  private shellPid: number;
  birth: string | undefined;
  private readonly dataListeners = new Set<(data: string) => void>();
  private readonly exitListeners = new Set<(event: { exitCode: number; signal?: number }) => void>();
  private readonly decoder = new StringDecoder('utf8');
  private pending = new Map<number, number>();
  private queued = 0;
  private nextId = 0;
  private ready = false;
  private ended = false;
  private confirmed = false;
  private closing = false;
  private failure: Error | undefined;
  private frame = Buffer.alloc(0);
  private resolveReady!: () => void;
  private rejectReady!: (error: Error) => void;
  private resolveClosed!: () => void;
  private rejectClosed!: (error: Error) => void;
  private readonly readiness: Promise<void>;
  private readonly closure: Promise<void>;
  private readonly timer: ReturnType<typeof setTimeout>;
  constructor(private readonly child: ChildProcessWithoutNullStreams, launch: ElevatedLaunch, timeoutMs = 125000) {
    this.shellPid = child.pid ?? 0;
    this.readiness = new Promise((resolve, reject) => { this.resolveReady = resolve; this.rejectReady = reject; });
    this.closure = new Promise((resolve, reject) => { this.resolveClosed = resolve; this.rejectClosed = reject; });
    // Both promises may reject before the backend reaches whenReady/close.
    void this.readiness.catch(() => {}); void this.closure.catch(() => {});
    child.stdout.on('data', (chunk: Buffer) => this.receive(chunk));
    child.stderr.resume(); // Helper never writes environment/input to diagnostics.
    child.on('error', () => this.fail('The administrator terminal launcher could not start.'));
    child.stdin.on('error', () => this.fail('The administrator terminal input channel disconnected.'));
    child.on('close', () => {
      clearTimeout(this.timer);
      if (!this.confirmed) {
        this.fail('Administrator terminal cleanup was not confirmed.');
        this.rejectClosed(this.failure!);
        this.finish(1, 1); // Transport exit is NOT elevated job exit evidence.
      }
    });
    this.timer = setTimeout(() => { this.fail('Administrator PowerShell approval timed out.'); this.kill(); }, timeoutMs);
    try { this.send({ type: 'launch', ...launch }); }
    catch (error) {
      clearTimeout(this.timer); child.stdin.destroy(); child.stdout.resume();
      this.fail('Administrator launch could not be sent.'); throw error;
    }
  }
  get pid(): number { return this.shellPid; }
  whenReady(): Promise<void> { return this.readiness; }
  queuedInputBytes(): number | null { return this.ready && !this.ended && !this.failure && !this.closing ? this.queued : null; }
  onData(listener: (data: string) => void) { this.dataListeners.add(listener); return { dispose: () => { this.dataListeners.delete(listener); } }; }
  onExit(listener: (event: { exitCode: number; signal?: number }) => void) {
    this.exitListeners.add(listener);
    // Backend also subscribes during explicit close after startup errors.
    if (this.ended) queueMicrotask(() => { if (this.exitListeners.has(listener)) listener({ exitCode: this.exitCode, signal: this.confirmed ? 0 : 1 }); });
    return { dispose: () => { this.exitListeners.delete(listener); } };
  }
  private exitCode = 1;
  private encode(value: unknown): string {
    const frame = JSON.stringify(value) + '\n';
    if (Buffer.byteLength(frame) > MAX_FRAME) throw new Error('Invalid or oversized helper frame.');
    return frame;
  }
  private sendFrame(frame: string): void {
    if (this.child.stdin.destroyed) throw new Error('Helper input disconnected.');
    this.child.stdin.write(frame);
  }
  private send(value: unknown): void { this.sendFrame(this.encode(value)); }
  write(data: string): void {
    const bytes = Buffer.byteLength(data);
    if (!this.ready || this.ended || this.failure || this.closing || !bytes || bytes > 65536 || data.includes('\0') || this.queued + bytes > MAX_INPUT || this.pending.size >= 128) throw new Error('Administrator input queue full or unavailable.');
    // Serialize and bound before reserving any input bytes or sequence ID.
    // Rejection of a large escaped frame leaves this terminal usable.
    const id = this.nextId + 1, frame = this.encode({ type: 'write', id, data });
    this.nextId = id; this.pending.set(id, bytes); this.queued += bytes;
    try { this.sendFrame(frame); }
    catch (error) { this.pending.delete(id); this.queued -= bytes; throw error; }
  }
  resize(cols: number, rows: number): void {
    if (!this.ready || this.ended || this.failure || this.closing || !Number.isInteger(cols) || cols < 2 || cols > 500 || !Number.isInteger(rows) || rows < 2 || rows > 500) throw new Error('Invalid administrator terminal resize.');
    // Bound all transport writes, not only text input. No deferred resize queue.
    if (this.child.stdin.writableLength > MAX_FRAME) throw new Error('Administrator transport is busy.');
    this.send({ type: 'resize', cols, rows });
  }
  kill(): void {
    if (this.confirmed || this.closing) return;
    this.closing = true;
    try { this.send({ type: 'close' }); }
    catch { this.child.stdin.destroy(); }
    // Never use taskkill/numeric PID kills, or kill the launcher before it can
    // report job cleanup. Host death closes stdio and helper pipe automatically.
  }
  async closeConfirmed(): Promise<void> {
    this.kill();
    let timer: ReturnType<typeof setTimeout> | undefined;
    try { await Promise.race([this.closure, new Promise<never>((_, reject) => { timer = setTimeout(() => reject(new Error('Elevated job cleanup timed out.')), 15000); })]); }
    finally { if (timer) clearTimeout(timer); }
  }
  private receive(chunk: Buffer): void {
    // One bounded accumulator. A child cannot make the host buffer an unbounded
    // line; several complete frames in a chunk do not count as one large frame.
    let start = 0;
    while (start < chunk.length) {
      const newline = chunk.indexOf(10, start);
      const end = newline < 0 ? chunk.length : newline;
      if (this.frame.length + end - start > MAX_FRAME) { this.abortProtocol(); return; }
      this.frame = Buffer.concat([this.frame, chunk.subarray(start, end)]);
      if (newline < 0) return;
      const frame = this.frame; this.frame = Buffer.alloc(0); start = newline + 1;
      try {
        const message = messageSchema.parse(JSON.parse(frame.toString('utf8')));
        if (this.confirmed) throw new Error('Message after exit.');
        switch (message.type) {
          case 'ready':
            if (this.ready) throw new Error('Unexpected ready.');
            this.ready = true; this.shellPid = message.pid; this.birth = message.birth;
            clearTimeout(this.timer);
            if (this.closing || this.failure) this.fail('Administrator PowerShell startup was cancelled.');
            else this.resolveReady();
            break;
          case 'data': {
            if (!this.ready) throw new Error('Output before ready.');
            const data = this.decoder.write(Buffer.from(message.data, 'base64'));
            if (data) for (const listener of this.dataListeners) listener(data);
            break;
          }
          case 'ack': {
            const bytes = this.pending.get(message.id);
            if (bytes === undefined || message.id !== this.pending.keys().next().value) throw new Error('Unexpected input acknowledgement.');
            this.pending.delete(message.id); this.queued -= bytes; break;
          }
          case 'error': this.fail(message.message); break;
          case 'exit':
            this.confirmed = true; clearTimeout(this.timer); this.resolveClosed();
            if (!this.ready) this.fail('Administrator PowerShell did not start.');
            this.finish(message.exitCode, 0); break;
        }
      } catch { this.abortProtocol(); return; }
    }
  }
  private fail(message: string): void { this.failure ??= new Error(message); this.rejectReady(this.failure); }
  private abortProtocol(): void {
    this.fail('Invalid administrator terminal protocol.');
    // Disconnecting causes the helper to close its job, but without a reply we
    // retain unconfirmed ownership rather than claiming successful cleanup.
    this.child.stdin.destroy(); this.child.stdout.destroy();
  }
  private finish(exitCode: number, signal: number): void {
    if (this.ended) return;
    const tail = this.decoder.end(); if (tail) for (const listener of this.dataListeners) listener(tail);
    this.ended = true; this.exitCode = exitCode; this.pending.clear(); this.queued = 0;
    for (const listener of this.exitListeners) listener({ exitCode, signal });
  }
}
export function elevatedLauncherEnvironment(inherited: NodeJS.ProcessEnv): Record<string, string> {
  // Applies both to helper startup and the final inherited+session shell data.
  // The privileged ConPTY constructor repeats this filter independently.
  return Object.fromEntries(Object.entries(inherited).filter((entry): entry is [string, string] => entry[1] !== undefined && !/^(?:DOTNET_|COMPLUS_|CORECLR_|COR_)/i.test(entry[0])));
}
export function launchElevatedPty(launch: ElevatedLaunch): ElevatedPty {
  launch = { ...launch, env: elevatedLauncherEnvironment(launch.env) };
  // Validate the whole serialized launch BEFORE acquiring a child process.
  if (Buffer.byteLength(JSON.stringify({ type: 'launch', ...launch }) + '\n') > MAX_FRAME) throw new Error('Administrator launch environment is too large.');
  const child = spawn(elevatedHelperPath(), ['launch'], { windowsHide: true, stdio: 'pipe', shell: false, env: elevatedLauncherEnvironment(process.env) });
  try { return new WindowsElevatedPty(child, launch); }
  catch (error) { child.stdin.destroy(); child.stdout.resume(); child.stderr.resume(); throw error; }
}
