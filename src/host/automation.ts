import type { ChildProcessWithoutNullStreams } from 'node:child_process';
import { launchAutomation } from './automation-launch';
import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { StringDecoder } from 'node:string_decoder';
import type { AutomationStart, AutomationRun, AutomationFrame } from '../shared/automation-schemas';
import type { PtyProcess } from '../main/terminal/backend';
import type { Result, SessionDto } from '../shared/contracts';

const CREDIT = 1024 * 1024;
const REPLAY = 256 * 1024;
interface Run {
  input: AutomationStart; owner: string; child: ChildProcessWithoutNullStreams; dto: AutomationRun;
  frames: AutomationFrame[]; bytes: number; sequence: number; acknowledged: number; capturing: boolean; lastRead: number;
  history: string; listeners: Set<(data: string) => void>; exits: Set<(event: { exitCode: number }) => void>;
  adapter: PtyProcess; inputEnded: boolean; stop?: Promise<void>;
}
export interface AutomationViewer {
  createAutomationTerminal?(input: { cwd: string; title?: string; requestId: string }, process: PtyProcess): Promise<Result<SessionDto>>;
  focusSession(input: { sessionId: string }): Promise<Result<unknown>> | Result<unknown>;
}
/** Local authenticated host jobs. Pipes remain separate from the lossy visual replay. */
export class AutomationRuns {
  private readonly runs = new Map<string, Run>();
  private readonly showing = new Map<string, Promise<AutomationRun>>();
  private closing = false;
  private readonly lease = setInterval(() => {
    for (const run of this.runs.values()) if (run.capturing && Date.now() - run.lastRead > 30_000) this.releasePipes(run);
  }, 5000);
  constructor(private readonly viewer: AutomationViewer, private readonly directory: string) { this.lease.unref(); }
  async start(input: AutomationStart, owner: string): Promise<AutomationRun> {
    if (this.closing) throw new Error('The host is shutting down.');
    const existing = this.runs.get(input.requestId);
    if (existing) {
      if (existing.owner !== owner || JSON.stringify(existing.input) !== JSON.stringify(input)) throw new Error('Run request ID already used.');
      return { ...existing.dto };
    }
    if (this.runs.size >= 64) {
      const retired = [...this.runs.values()].find(run => run.dto.state === 'exited' && !run.frames.length);
      if (retired) this.runs.delete(retired.dto.id);
      else throw new Error('The host has 64 retained automation runs.');
    }
    if (!path.isAbsolute(input.cwd)) throw new Error('Automation cwd must be absolute.');
    const launch = launchAutomation(input, this.directory), child = launch.child;
    const dto: AutomationRun = { id: input.requestId, pid: child.pid ?? 0, executable: input.executable, cwd: input.cwd, title: (input.title ?? path.basename(input.executable)).slice(0, 200), state: 'running', exitCode: null, signal: null, sessionId: null, tabId: null };
    const run: Run = { input, owner, child, dto, frames: [], bytes: 0, sequence: 0, acknowledged: 0, capturing: !input.detached, lastRead: Date.now(), history: '', listeners: new Set(), exits: new Set(), adapter: this.adapter(dto.id), inputEnded: input.detached };
    this.runs.set(dto.id, run);
    const decoders = { stdout: new StringDecoder('utf8'), stderr: new StringDecoder('utf8') };
    for (const stream of ['stdout', 'stderr'] as const) child[stream].on('data', (chunk: Buffer) => {
      this.display(run, decoders[stream].write(chunk));
      if (!run.capturing) return;
      for (let offset = 0; offset < chunk.length; offset += 32768) {
        const bytes = chunk.subarray(offset, offset + 32768);
        run.frames.push({ sequence: ++run.sequence, stream, data: bytes.toString('base64') }); run.bytes += bytes.length;
      }
      if (run.bytes >= CREDIT) { child.stdout.pause(); child.stderr.pause(); }
    });
    child.stdin.on('error', () => { run.inputEnded = true; });
    child.once('close', (code, signal) => {
      this.display(run, decoders.stdout.end() + decoders.stderr.end());
      const outcome = launch.outcome();
      dto.state = 'exited'; dto.exitCode = outcome ? outcome.code : code; dto.signal = outcome ? outcome.signal : signal;
      for (const listener of run.exits) listener({ exitCode: dto.exitCode ?? 1 });
    });
    try {
      await launch.ready;
      dto.pid = child.pid ?? 0;
      if (input.detached) child.stdin.end();
      return { ...dto };
    } catch (error) { this.runs.delete(dto.id); throw error; }
  }
  list(): AutomationRun[] { return [...this.runs.values()].map(run => ({ ...run.dto })); }
  releaseOwner(owner: string): void {
    for (const run of this.runs.values()) if (run.owner === owner) this.releasePipes(run);
  }
  private releasePipes(run: Run): void {
    run.capturing = false; run.frames = []; run.bytes = 0;
    run.child.stdout.resume(); run.child.stderr.resume();
    if (!run.inputEnded) { run.inputEnded = true; run.child.stdin.end(); }
  }
  private get(id: string): Run { const run = this.runs.get(id); if (!run) throw new Error('Automation run not found.'); return run; }
  private owned(id: string, owner: string): Run {
    const run = this.get(id); if (run.owner !== owner) throw new Error('Only the starting client can use this run\'s pipes.'); return run;
  }
  read(id: string, after: number, owner: string): { frames: AutomationFrame[]; run: AutomationRun } {
    const run = this.owned(id, owner);
    if (!run.capturing) throw new Error('Pipe collection ended. The process continues; use runs show to inspect its bounded visual output.');
    if (after < run.acknowledged || after > run.sequence) throw new Error('Invalid automation output cursor.');
    run.lastRead = Date.now(); run.acknowledged = after;
    while (run.frames.length && run.frames[0].sequence <= after) { const frame = run.frames.shift()!; run.bytes -= Buffer.byteLength(frame.data, 'base64'); }
    if (run.bytes < CREDIT) { run.child.stdout.resume(); run.child.stderr.resume(); }
    return { frames: run.frames.slice(0, 16), run: { ...run.dto } };
  }
  async write(id: string, data: string, eof: boolean, owner: string): Promise<{ accepted: true }> {
    const run = this.owned(id, owner);
    if (run.inputEnded || run.dto.state !== 'running') throw new Error('The process stdin is closed.');
    const bytes = Buffer.from(data, 'base64');
    if (bytes.length) await new Promise<void>((resolve, reject) => { run.child.stdin.write(bytes, error => error ? reject(error) : resolve()); });
    if (eof) { run.inputEnded = true; run.child.stdin.end(); }
    return { accepted: true };
  }
  private display(run: Run, text: string): void {
    if (!text) return;
    // The view is a terminal, while the original pipe bytes remain untouched.
    const display = text.replace(/\r?\n/g, '\r\n');
    run.history = (run.history + display).slice(-REPLAY);
    for (const listener of run.listeners) listener(display);
  }
  private adapter(id: string): PtyProcess {
    return {
      get pid() { return 0; }, // Replaced below when the view is created after spawn.
      write: () => { throw new Error('Automation viewers are read-only. Send input through the starting command.'); },
      resize: () => {}, kill: () => { void this.stop(id).catch(() => {}); }, queuedInputBytes: () => 0,
      onData: listener => {
        const run = this.get(id); let disposed = false;
        run.listeners.add(listener);
        const history = run.history;
        queueMicrotask(() => { if (!disposed && history) listener(history); });
        return { dispose: () => { disposed = true; run.listeners.delete(listener); } };
      },
      onExit: listener => {
        const run = this.get(id); let disposed = false;
        run.exits.add(listener);
        if (run.dto.state === 'exited') queueMicrotask(() => { if (!disposed) listener({ exitCode: run.dto.exitCode ?? 1 }); });
        return { dispose: () => { disposed = true; run.exits.delete(listener); } };
      },
    };
  }
  async show(id: string): Promise<AutomationRun> {
    const existing = this.showing.get(id); if (existing) return existing;
    const task = this.showRun(id); this.showing.set(id, task);
    try { return await task; } finally { this.showing.delete(id); }
  }
  private async showRun(id: string): Promise<AutomationRun> {
    const run = this.get(id);
    if (!run.dto.sessionId) {
      if (!this.viewer.createAutomationTerminal) throw new Error('Automation viewing is unavailable on this host.');
      const adapter = { ...run.adapter, pid: run.dto.pid };
      const result = await this.viewer.createAutomationTerminal({ cwd: run.dto.cwd, title: run.dto.title, requestId: randomUUID() }, adapter);
      if (!result.ok) throw new Error(result.error.message);
      run.dto.sessionId = result.value.id; run.dto.tabId = result.value.tabs[0]?.id ?? null;
    }
    const focused = await this.viewer.focusSession({ sessionId: run.dto.sessionId });
    if (!focused.ok) throw new Error(focused.error.message);
    return { ...run.dto };
  }
  async stop(id: string): Promise<{ stopped: true }> {
    const run = this.get(id);
    if (run.dto.state === 'exited') return { stopped: true };
    run.stop ??= (async () => {
      if (process.platform === 'win32') {
        // libuv holds the helper's process handle; killing it closes its native job.
        if (!run.child.kill('SIGKILL') && run.child.exitCode === null && run.child.signalCode === null) throw new Error('Automation supervisor termination failed.');
      } else if (run.child.exitCode === null && run.child.signalCode === null) {
        // The live supervisor still owns this process-group identity.
        try { process.kill(-run.dto.pid, 'SIGKILL'); } catch (error) { if (!(error instanceof Error && 'code' in error && error.code === 'ESRCH')) throw error; }
      }
      run.child.stdout.resume(); run.child.stderr.resume();
      if (run.dto.state === 'running') await new Promise<void>((resolve, reject) => {
        const timer = setTimeout(() => { run.child.off('close', done); reject(new Error('Automation process exit was not confirmed.')); }, 10000);
        const done = () => { clearTimeout(timer); resolve(); }; run.child.once('close', done);
      });
    })().catch(error => { run.stop = undefined; throw error; });
    await run.stop; return { stopped: true };
  }
  async close(): Promise<void> {
    this.closing = true; clearInterval(this.lease);
    await Promise.all([...this.runs.keys()].map(id => this.stop(id)));
  }
}
