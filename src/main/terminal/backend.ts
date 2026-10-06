import { createRequire } from 'node:module';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import type { EnvVar, Result, TerminalAttachmentDto, TerminalEvent, TerminalProfileDto, TerminalProfilesDto } from '../../shared/contracts';
import { failure, success } from '../../shared/contracts';
import { idSchema, pathSchema, profileIdSchema, requestSchemas } from '../../shared/schemas';
import { z } from 'zod';
import { discoverProfiles, spawnArguments, validateProfileCwd } from './profiles';
import type { TrackingRoot } from './tracking';
import { captureLinuxRoot, closeUnixTree, type UnixIdentity } from './unix-close';
import { terminalEnvironment } from './environment';
import { envVarsSchema } from '../../shared/schemas';
import { TerminalActivity } from './activity';
import { nativeQueuedInputBytes, MAX_NATIVE_INPUT_BYTES } from './native-input';
import { prepareDarwinSupervisor, type SupervisorLaunch, type SupervisorControl } from './darwin-supervisor';

export const REPLAY_BYTES = 256 * 1024;
const GLOBAL_REPLAY_BYTES = 16 * 1024 * 1024;
const MAX_CHUNKS = 8192;
export interface PtyProcess {
  pid: number; write(data: string): void; resize(cols: number, rows: number): void; kill(signal?: string): void;
  onData(listener: (data: string) => void): { dispose(): void };
  onExit(listener: (event: { exitCode: number; signal?: number }) => void): { dispose(): void };
  queuedInputBytes?(): number | null;
  whenReady?(): Promise<void>;
}
export type PtyFactory = (file: string, args: string[], options: { name: string; cols: number; rows: number; cwd?: string; env: Record<string, string>; useConpty: boolean; useConptyDll?: boolean }) => PtyProcess;
export interface TerminalLaunch { tabId: string; sessionId: string; generation: string; profileId: string; cwd: string; env?: EnvVar[]; cliBin?: string }
export interface OwnedTerminal {
  tabId: string; sessionId: string; generation: string; profileId: string; cwd: string;
  state: 'open' | 'closed'; exitCode: number | null; cols: number; rows: number; root: TrackingRoot;
  cleanupPending: boolean;
}
interface Entry extends OwnedTerminal {
  activity: TerminalActivity;
  pty: PtyProcess; chunks: { event: Extract<TerminalEvent, { type: 'data' }>; bytes: number; order: number }[];
  bytes: number; sequence: number; inputBytes: number; inputWindow: number;
  handles: { dispose(): void }[]; closing?: Promise<Result<{ closed: true }>>;
  identity: UnixIdentity | null; known: UnixIdentity[]; ready: boolean;
  supervisor?: SupervisorControl;
}
function defaultFactory(): PtyFactory {
  // Deliberately external/lazy: the packaging owner stages the Electron-ABI native addon.
  const require = createRequire(typeof __filename === 'string' ? __filename : path.join(process.cwd(), 'package.json'));
  const module = require('node-pty') as { spawn: PtyFactory };
  return (file, args, options) => {
    const raw = module.spawn(file, args, options);
    return { get pid() { return raw.pid; }, write: data => raw.write(data), resize: (cols, rows) => raw.resize(cols, rows), kill: signal => raw.kill(signal), onData: fn => raw.onData(fn), onExit: fn => raw.onExit(fn),
      queuedInputBytes: () => nativeQueuedInputBytes(raw),
      whenReady: async () => { const start = Date.now(); while (nativeQueuedInputBytes(raw) === null && Date.now() - start < 5000) await new Promise(resolve => setTimeout(resolve, 10)); if (nativeQueuedInputBytes(raw) === null) throw new Error('Native input queue cannot be inspected.'); },
    };
  };
}
export interface BackendOptions {
  factory?: PtyFactory; discover?: () => Promise<TerminalProfilesDto>;
  validateCwd?: (profile: TerminalProfileDto, cwd: string) => Promise<string>;
  terminateGuest?: TreeTerminator;
  terminateUnix?: TreeTerminator;
  closeTimeoutMs?: number;
  platform?: NodeJS.Platform;
  prepareSupervisor?: (profile: TerminalProfileDto, cwd: string, marker: string) => Promise<SupervisorLaunch>;
}
export type TreeTerminator = (root: TrackingRoot, identity: UnixIdentity | null, known: UnixIdentity[]) => Promise<void>;
export class PtyBackend {
  readonly lifetime = 'app-owned';
  private factory?: PtyFactory;
  private terminateGuest?: TreeTerminator;
  private readonly terminateUnix: TreeTerminator;
  private readonly platform: NodeJS.Platform;
  private profiles: TerminalProfilesDto = { profiles: [], defaultProfileId: null, lifetime: 'app-owned', shellSurvival: false };
  private readonly entries = new Map<string, Entry>();
  private readonly listeners = new Set<(event: TerminalEvent) => void>();
  private disposed = false;
  private initialized = false;
  private order = 0;
  private replayBytes = 0;
  constructor(private readonly options: BackendOptions = {}) { this.factory = options.factory; this.terminateGuest = options.terminateGuest; this.terminateUnix = options.terminateUnix ?? closeUnixTree; this.platform = options.platform ?? process.platform; }
  configureGuestTermination(close: TreeTerminator): void { this.terminateGuest ??= close; }
  rememberOwnership(tabId: string, generation: string, identity: UnixIdentity | null, known: UnixIdentity[]): void {
    const e = this.entries.get(tabId); if (!e || e.generation !== generation) return;
    if (identity) e.identity ??= { pid: identity.pid, birth: identity.birth };
    e.known = known.map(p => ({ pid: p.pid, birth: p.birth }));
  }
  async initialize(): Promise<Result<TerminalProfilesDto>> {
    if (this.disposed) return failure('NATIVE_UNAVAILABLE', 'Terminal backend disposed.');
    if (this.initialized) return success(this.getProfiles());
    try {
      this.factory ??= defaultFactory();
      this.profiles = await (this.options.discover ?? discoverProfiles)();
      this.initialized = true;
      return success(this.getProfiles());
    } catch { return failure('DEPENDENCY_MISSING', 'The embedded terminal addon or discovered shell is unavailable. Rebuild node-pty for this Electron runtime.'); }
  }
  getProfiles(): TerminalProfilesDto { return structuredClone(this.profiles); }
  get(tabId: string): OwnedTerminal | undefined {
    const e = this.entries.get(tabId);
    return e && { tabId: e.tabId, sessionId: e.sessionId, generation: e.generation, profileId: e.profileId, cwd: e.cwd, state: e.state, exitCode: e.exitCode, cols: e.cols, rows: e.rows, root: { ...e.root }, cleanupPending: e.cleanupPending };
  }
  live(): OwnedTerminal[] { return [...this.entries.keys()].map(id => this.get(id)!).filter(e => e.state === 'open' || e.cleanupPending); }
  async awaitCleanup(sessionId: string): Promise<Result<{ confirmed: true }>> {
    const pending = [...this.entries.values()].filter(e => e.sessionId === sessionId && e.state === 'closed' && e.cleanupPending);
    for (const e of pending) { if (!e.closing) return failure('RETRY_CONFIRM_REQUIRED', 'Owned descendant cleanup is unconfirmed; no replacement shell was launched.'); const result = await e.closing; if (!result.ok) return result; }
    return success({ confirmed: true });
  }
  subscribe(listener: (event: TerminalEvent) => void): () => void { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; }
  private emit(event: TerminalEvent): void { for (const listener of this.listeners) { try { listener(event); } catch { /* Views cannot break PTY ownership. */ } } }
  async launch(input: TerminalLaunch): Promise<Result<OwnedTerminal>> {
    if (!z.object({ tabId: idSchema, sessionId: idSchema, generation: idSchema, profileId: profileIdSchema, cwd: pathSchema, env: envVarsSchema.optional(), cliBin: pathSchema.optional() }).strict().safeParse(input).success) return failure('VALIDATION', 'Invalid embedded launch identity.');
    if (this.disposed || !this.initialized || !this.factory) return failure('NATIVE_UNAVAILABLE', 'Embedded terminal backend is not ready.');
    const prior = this.entries.get(input.tabId);
    if (prior?.generation === input.generation) return success(this.get(input.tabId)!);
    if (prior?.state === 'open' || prior?.cleanupPending) return failure('VALIDATION', 'This tab already owns a live shell or unconfirmed cleanup.');
    if (this.live().length >= 64) return failure('VALIDATION', 'The live embedded terminal limit is 64.');
    const profile = this.profiles.profiles.find(p => p.id === input.profileId && p.available);
    if (!profile) return failure('DEPENDENCY_MISSING', 'Choose a discovered terminal profile.');
    let cwd: string;
    try { cwd = await (this.options.validateCwd ?? validateProfileCwd)(profile, input.cwd); }
    catch { return failure('VALIDATION', 'Choose an accessible directory in the selected shell environment.'); }
    if (this.disposed) return failure('NATIVE_UNAVAILABLE', 'Terminal backend is shutting down.');
    // A second launch may have won while directory validation was awaiting the OS.
    let current = this.entries.get(input.tabId);
    if (current?.generation === input.generation) return success(this.get(input.tabId)!);
    if (current?.state === 'open' || current?.cleanupPending) return failure('VALIDATION', 'This tab already owns a live shell or unconfirmed cleanup.');
    if (this.live().length >= 64) return failure('VALIDATION', 'The live embedded terminal limit is 64.');
    const marker = randomUUID();
    let supervisor: SupervisorLaunch | undefined;
    if (this.platform === 'darwin' && profile.environment === 'local') {
      try { supervisor = await (this.options.prepareSupervisor ?? prepareDarwinSupervisor)(profile, cwd, marker); }
      catch { return failure('DEPENDENCY_MISSING', 'The bundled Darwin session supervisor failed native preflight or private control setup.'); }
    }
    // Native preflight/private endpoint preparation awaited again. Recheck all
    // ownership and capacity guards before spawning, not the earlier snapshot.
    current = this.entries.get(input.tabId);
    if (this.disposed || current?.generation === input.generation || current?.state === 'open' || current?.cleanupPending || this.live().length >= 64) {
      await supervisor?.control.disposeConfirmed();
      if (this.disposed) return failure('NATIVE_UNAVAILABLE', 'Terminal backend is shutting down.');
      if (current?.generation === input.generation) return success(this.get(input.tabId)!);
      return failure('VALIDATION', 'Terminal ownership changed or the live terminal limit was reached during preparation.');
    }
    const command = supervisor ?? spawnArguments(profile, cwd, marker);
    const env = terminalEnvironment({ ...process.env, TERM: 'xterm-256color', COLORTERM: 'truecolor' }, input.env ?? [], { windows: this.platform === 'win32', wsl: profile.environment === 'wsl', cliBin: input.cliBin });
    for (const name of Object.keys(env)) if (name.toLowerCase() === 'shellfox_terminal_marker') delete env[name];
    env.SHELLFOX_TERMINAL_MARKER = marker;
    if (supervisor) Object.assign(env, supervisor.env);
    let pty: PtyProcess;
    const bornBefore = (BigInt(Date.now()) + 11644473600000n) * 10000n;
    try { pty = this.factory(command.file, command.args, { name: 'xterm-256color', cols: 80, rows: 24, cwd: command.cwd, env, useConpty: true, useConptyDll: true }); }
    catch { await supervisor?.control.disposeConfirmed(); return failure('LAUNCH_FAILED', 'The selected interactive shell could not start.'); }
    const bornAfter = (BigInt(Date.now()) + 11644473600000n) * 10000n + 9999n;
    if (current) { current.activity.stop(); this.replayBytes -= current.bytes; current.handles.forEach(h => h.dispose()); }
    const e: Entry = { ...input, activity: new TerminalActivity(input.tabId, event => this.emit(event)), cwd, state: 'open', exitCode: null, cols: 80, rows: 24, pty, root: { tabId: input.tabId, sessionId: input.sessionId, generation: input.generation, pid: pty.pid, environment: profile.environment, distro: profile.distro, marker, shellExecutable: profile.environment === 'wsl' ? '/bin/bash' : profile.executable, ...(this.platform === 'win32' && profile.environment === 'local' ? { ancestorPid: process.pid, ...(bornBefore <= bornAfter ? { birthOrderBounds: { min: bornBefore.toString(), max: bornAfter.toString() } } : {}) } : {}) }, chunks: [], bytes: 0, sequence: 0, inputBytes: 0, inputWindow: Date.now(), handles: [], identity: null, known: [], ready: false, cleanupPending: profile.environment === 'wsl' || this.platform !== 'win32', supervisor: supervisor?.control };
    this.entries.set(input.tabId, e);
    try {
      e.handles.push(pty.onData(data => this.output(e, data)));
      e.handles.push(pty.onExit(exit => this.exited(e, exit.exitCode, exit.signal ?? null)));
    } catch {
      // Retain ownership if event installation fails. A kill call alone is not exit evidence.
      if (this.platform === 'win32' && e.root.environment === 'local') { try { pty.kill(); } catch { /* The handle remains owned for explicit retry. */ } }
      return failure('LAUNCH_FAILED', 'The PTY event handlers could not be attached. Closure is not confirmed; its handle remains owned.');
    }
    if (supervisor) {
      try {
        const status = await supervisor.control.ready(pty.pid);
        // Tracking follows the actual interactive shell, not the native supervisor transport.
        e.root.pid = status.shellPid; e.root.authenticatedBirth = status.shellBirth;
        e.identity = { pid: status.shellPid, birth: status.shellBirth };
      } catch { return failure('LAUNCH_FAILED', 'Darwin shell startup could not be authenticated. Its supervisor remains owned for explicit close/retry.'); }
    }
    if (!this.options.factory && this.platform === 'linux' && profile.environment === 'local') e.identity = await captureLinuxRoot(e.root);
    try { await pty.whenReady?.(); e.ready = true; }
    catch { return failure('NATIVE_UNAVAILABLE', 'The PTY input channel cannot safely report outstanding writes. Its owned handle was retained.'); }
    return success(this.get(e.tabId)!);
  }
  private output(e: Entry, data: string): void {
    if (e.state !== 'open' || this.entries.get(e.tabId) !== e || !data) return;
    e.activity.output();
    // Split UTF-16 on codepoint boundaries; 4096 units are always <=16 KiB UTF-8.
    for (let offset = 0; offset < data.length;) {
      let end = Math.min(data.length, offset + 4096);
      if (end < data.length && /[\uD800-\uDBFF]/.test(data[end - 1]!)) end--;
      const part = data.slice(offset, end); offset = end;
      const bytes = Buffer.byteLength(part);
      const event = { type: 'data' as const, tabId: e.tabId, generation: e.generation, sequence: ++e.sequence, data: part };
      e.chunks.push({ event, bytes, order: ++this.order }); e.bytes += bytes; this.replayBytes += bytes;
      while (e.bytes > REPLAY_BYTES || e.chunks.length > MAX_CHUNKS) this.evict(e);
      while (this.replayBytes > GLOBAL_REPLAY_BYTES) {
        const oldest = [...this.entries.values()].filter(p => p.chunks.length).sort((a, b) => a.chunks[0].order - b.chunks[0].order)[0];
        if (!oldest) break;
        this.evict(oldest);
      }
      this.emit(event);
    }
  }
  private evict(e: Entry): void { const removed = e.chunks.shift(); if (removed) { e.bytes -= removed.bytes; this.replayBytes -= removed.bytes; } }
  private exited(e: Entry, exitCode: number | null, signal: number | null): void {
    if (this.entries.get(e.tabId) !== e || e.state === 'closed') return;
    e.state = 'closed'; e.exitCode = exitCode;
    e.activity.stop();
    // Trusted native supervisor normal exit occurs only after verified cleanup or before any child launch.
    // External SIGKILL/crash is not that contract and keeps cleanup ownership unconfirmed.
    if (e.supervisor && (signal === null || signal === 0)) { e.cleanupPending = false; void e.supervisor.disposeConfirmed(); }
    this.emit({ type: 'exit', tabId: e.tabId, generation: e.generation, exitCode, signal, lastSequence: e.sequence });
    if (e.cleanupPending && !e.closing) void this.close({ tabId: e.tabId, generation: e.generation }).then(result => { if (!result.ok) this.emit({ type: 'error', tabId: e.tabId, generation: e.generation, error: result.error }); });
    const closed = [...this.entries.values()].filter(p => p.state === 'closed' && !p.cleanupPending);
    while (closed.length > 256) {
      const retired = closed.shift()!;
      if (retired === e) continue;
      retired.handles.forEach(h => h.dispose()); this.replayBytes -= retired.bytes; this.entries.delete(retired.tabId);
    }
  }
  attach(input: { tabId: string; afterSequence?: number; generation?: string }): Result<TerminalAttachmentDto> {
    if (!requestSchemas.attachTerminal.safeParse(input).success) return failure('VALIDATION', 'Invalid terminal attachment.');
    const e = this.entries.get(input.tabId);
    if (!e) return failure('NOT_FOUND', 'No runtime terminal exists for this tab. Opening a session creates a fresh shell.');
    const mismatch = !!input.generation && input.generation !== e.generation;
    const after = mismatch ? 0 : input.afterSequence ?? 0;
    if (after > e.sequence) return failure('VALIDATION', 'The requested sequence is ahead of this terminal.');
    const first = e.chunks[0]?.event.sequence ?? e.sequence + 1;
    return success({ tabId: e.tabId, sessionId: e.sessionId, generation: e.generation, firstSequence: first, lastSequence: e.sequence, chunks: e.chunks.filter(c => c.event.sequence > after).map(c => ({ ...c.event })), truncated: mismatch || after < first - 1, state: e.state, exitCode: e.exitCode, cols: e.cols, rows: e.rows, lifetime: 'app-owned' });
  }
  private owned(tabId: string, generation: string): Result<Entry> {
    const e = this.entries.get(tabId);
    if (!e) return failure('NOT_FOUND', 'This tab does not own an embedded terminal.');
    if (e.generation !== generation) return failure('TARGET_LOST', 'This terminal generation is stale.');
    return success(e);
  }
  write(input: { tabId: string; generation: string; data: string }): Result<{ written: true }> {
    if (!requestSchemas.writeTerminal.safeParse(input).success) return failure('VALIDATION', 'Invalid or oversized terminal input.');
    const owned = this.owned(input.tabId, input.generation); if (!owned.ok) return owned;
    const e = owned.value;
    if (e.state !== 'open') return failure('TARGET_LOST', 'This shell has exited.');
    if (e.closing) return failure('TARGET_LOST', 'Owned terminal cleanup is in progress; no input was enqueued.');
    if (!e.ready) return failure('NATIVE_UNAVAILABLE', 'The terminal input channel is still being prepared.');
    const queued = e.pty.queuedInputBytes?.();
    if (queued === undefined || queued === null || !Number.isSafeInteger(queued) || queued < 0) return failure('NATIVE_UNAVAILABLE', 'The native input queue cannot be bounded safely. Input was not enqueued.');
    if (queued + Buffer.byteLength(input.data) > MAX_NATIVE_INPUT_BYTES) return failure('VALIDATION', 'Outstanding native terminal input exceeds 128 KiB. Wait for the shell to consume input before retrying.');
    if (Date.now() - e.inputWindow >= 1000) { e.inputWindow = Date.now(); e.inputBytes = 0; }
    const bytes = Buffer.byteLength(input.data);
    if (e.inputBytes + bytes > 256 * 1024) return failure('VALIDATION', 'Terminal input rate exceeded 256 KiB per second.');
    try { e.activity.input(); e.pty.write(input.data); e.inputBytes += bytes; return success({ written: true }); }
    catch { return failure('NATIVE_UNAVAILABLE', 'Terminal input could not be written.'); }
  }
  resize(input: { tabId: string; generation: string; cols: number; rows: number }): Result<{ resized: true }> {
    if (!requestSchemas.resizeTerminal.safeParse(input).success) return failure('VALIDATION', 'Invalid terminal dimensions.');
    const owned = this.owned(input.tabId, input.generation); if (!owned.ok) return owned;
    const e = owned.value; if (e.state !== 'open') return failure('TARGET_LOST', 'This shell has exited.');
    if (e.closing) return failure('TARGET_LOST', 'Owned terminal cleanup is in progress; no resize was enqueued.');
    if (!e.ready) return failure('NATIVE_UNAVAILABLE', 'The terminal channel is still being prepared. No deferred resize was enqueued.');
    try { e.pty.resize(input.cols, input.rows); e.cols = input.cols; e.rows = input.rows; return success({ resized: true }); }
    catch { return failure('NATIVE_UNAVAILABLE', 'Terminal could not be resized.'); }
  }
  async close(input: { tabId: string; generation: string }): Promise<Result<{ closed: true }>> {
    if (!requestSchemas.closeTab.safeParse(input).success) return failure('VALIDATION', 'Invalid terminal identity.');
    const owned = this.owned(input.tabId, input.generation); if (!owned.ok) return owned;
    const e = owned.value; if (e.state === 'closed' && !e.cleanupPending) return success({ closed: true });
    if (e.closing) return e.closing;
    e.closing = Promise.resolve().then(() => this.closeOwned(e)).finally(() => { e.closing = undefined; });
    return e.closing;
  }
  private async closeOwned(e: Entry): Promise<Result<{ closed: true }>> {
    if (e.supervisor) {
      try { await e.supervisor.close(); e.cleanupPending = false; }
      catch {
        // Natural verified cleanup can unlink the endpoint while CLOSE is in
        // flight. Its trusted normal PTY exit is still authoritative evidence.
        if (e.state === 'closed' && !e.cleanupPending) return success({ closed: true });
        return failure('MONITOR_UNAVAILABLE', 'The native Darwin supervisor did not confirm owned-session cleanup. It was not killed; ownership is retained for retry.', true);
      }
    }
    else if (e.root.environment === 'wsl') {
      try { if (!this.terminateGuest) throw new Error('No guest termination provider.'); await this.terminateGuest(e.root, e.identity, e.known); e.cleanupPending = false; }
      catch { return failure('MONITOR_UNAVAILABLE', 'Guest shell termination could not be verified. Keep the manager open and exit the guest shell manually, or restore guest tracking and Python pidfd support.', true); }
    }
    else if (this.platform !== 'win32') {
      try { await this.terminateUnix(e.root, e.identity, e.known); e.cleanupPending = false; }
      catch { return failure('MONITOR_UNAVAILABLE', 'Owned Unix descendant termination could not be confirmed. No numeric PID-only kill was attempted; ownership remains pending for retry.', true); }
    }
    if (e.state === 'closed') return success({ closed: true });
    return new Promise(resolve => {
      let handle: { dispose(): void } | undefined, done = false;
      const finish = (result: Result<{ closed: true }>) => { if (done) return; done = true; clearTimeout(timer); handle?.dispose(); resolve(result); };
      const timer = setTimeout(() => finish(failure('NATIVE_UNAVAILABLE', 'The owned PTY did not confirm exit. Its handle remains owned; no shell survival or closure is claimed.', true)), this.options.closeTimeoutMs ?? 5000);
      try { handle = e.pty.onExit(() => finish(success({ closed: true }))); if (this.platform === 'win32') e.pty.kill(); }
      catch { finish(failure('NATIVE_UNAVAILABLE', 'The owned PTY could not be terminated. Its handle was retained for a retry.', true)); }
    });
  }
  async dispose(): Promise<void> {
    if (this.disposed) return;
    this.disposed = true;
    const results = await Promise.all(this.live().map(e => this.close({ tabId: e.tabId, generation: e.generation })));
    if (results.some(r => !r.ok)) { this.disposed = false; throw new Error('Owned terminal termination was not confirmed.'); }
    for (const e of this.entries.values()) { e.activity.stop(); e.handles.forEach(h => h.dispose()); }
    this.entries.clear(); this.listeners.clear(); this.replayBytes = 0;
  }
}
