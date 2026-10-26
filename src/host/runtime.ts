import type { HostMode, HostStatus } from '../shared/host-contracts';
import type { SshProfileStore } from '../main/ssh/storage';
import { acquireHostOwnership, type HostOwnership, type HostOwnershipOptions } from './ownership';
import { startHostRpc, type HostRemoteService, type HostRpc } from './rpc';
import type { HostKeepAwake, HostSessionService } from './operations';
import { createHostResources } from './resources';
import { getHostStatus } from './status';
import type { HostCliBinding } from './cli-binding';

export interface HostResources {
  service: HostSessionService & { dispose(options?: { fatal?: boolean }): Promise<void> };
  remote?: HostRemoteService & { dispose(): Promise<void> };
  keepAwake?: HostKeepAwake & { dispose(): void | Promise<void> };
  ssh?: SshProfileStore;
  initialize(): Promise<void>;
  closeRepository(): void;
  startLegacyControl?(): Promise<{ dispose(): Promise<void> }>;
}
export interface HostRuntimeOptions extends HostOwnershipOptions {
  userData: string; mode?: HostMode; cliBinding?: HostCliBinding;
  onReady?(status: HostStatus): void;
  assertOwnership?(): void;
  /** Tests or an embedding desktop may provide already constructed, uninitialized resources. */
  createResources?(options: HostRuntimeOptions): Promise<HostResources>;
}
export class HostRuntime {
  readonly startedAt = new Date().toISOString();
  private rpc?: HostRpc;
  private control?: { dispose(): Promise<void> };
  private stopping?: Promise<void>;
  private stopped = false;
  private notified = false;
  private ready = false;
  private completionError?: Error;
  private readonly unsubscribeOwnership: () => void;
  private constructor(readonly ownership: HostOwnership, readonly resources: HostResources, readonly mode: HostMode) {
    this.unsubscribeOwnership = ownership.onLost(() => {
      if (this.ready) void this.stop().catch(error => { this.completionError ??= error instanceof Error ? error : new Error('Fatal host shutdown failed.', { cause: error }); });
    });
  }
  static async create(options: HostRuntimeOptions): Promise<HostRuntime> {
    const mode = options.mode ?? 'headless';
    const ownership = await acquireHostOwnership(options.userData, mode, options);
    if (!ownership) throw new Error('Another Shellfox host already owns this user data directory. Attach to it instead.');
    let resources: HostResources;
    try { resources = await (options.createResources ?? createHostResources)({ ...options, userData: ownership.userData, mode, assertOwnership: () => ownership.assertOwned() }); }
    catch (error) { await ownership.release(); throw error; }
    const runtime = new HostRuntime(ownership, resources, mode);
    try {
      ownership.assertOwned(); await resources.initialize(); ownership.assertOwned();
      runtime.control = await resources.startLegacyControl?.(); ownership.assertOwned();
      runtime.rpc = await startHostRpc({ userData: ownership.userData, mode, ...resources, guard: () => ownership.assertOwned(), startedAt: runtime.startedAt, shutdown: () => runtime.stop() });
      ownership.assertOwned(); runtime.ready = true;
      return runtime;
    } catch (error) { await runtime.stop(); throw error; }
  }
  status(): HostStatus {
    return getHostStatus(this.mode, this.startedAt, this.resources.service, this.resources.keepAwake);
  }
  private readonly listeners = new Set<(error?: Error) => void>();
  onStopped(listener: (error?: Error) => void): () => void { if (this.notified) listener(this.completionError); else this.listeners.add(listener); return () => { this.listeners.delete(listener); }; }
  stop(): Promise<void> {
    if (this.stopped) return this.completionError ? Promise.reject(this.completionError) : Promise.resolve();
    return this.stopping ??= this.shutdown().catch(error => { this.stopping = undefined; throw error; });
  }
  private complete(error?: Error): void {
    this.stopped = true; this.completionError = error;
    const notify = () => {
      this.notified = true;
      for (const listener of this.listeners) { try { listener(this.completionError); } catch { /* Observers cannot reverse completed shutdown. */ } }
      this.listeners.clear();
    };
    // stop() must resolve first so its RPC handler can write the final response.
    // Only foreground completion waits for that response to flush before process exit.
    void (this.rpc?.waitForStopReplies() ?? Promise.resolve()).then(notify);
  }
  private async shutdown(): Promise<void> {
    if (this.ownership.lost) return this.shutdownAfterLoss(this.ownership.lost);
    try {
      // Failed PTY cleanup restores the service. Keep its APIs reachable for inspect/retry.
      await this.resources.service.dispose();
      await Promise.all([this.rpc?.close(), this.control?.dispose()]);
      await this.resources.remote?.dispose(); await this.resources.keepAwake?.dispose();
      this.ownership.assertOwned();
      this.resources.closeRepository(); this.unsubscribeOwnership(); await this.ownership.release(); this.complete();
    } catch (error) {
      if (this.ownership.lost) return this.shutdownAfterLoss(this.ownership.lost, error);
      throw error;
    }
  }
  private async shutdownAfterLoss(cause: Error, previousError?: unknown): Promise<never> {
    const errors: unknown[] = [cause]; if (previousError !== undefined) errors.push(previousError);
    const attempt = async (action: () => void | Promise<void>) => { try { await action(); } catch (error) { errors.push(error); } };
    await Promise.all([attempt(async () => { await this.rpc?.close(); }), attempt(async () => { await this.control?.dispose(); })]);
    await attempt(() => this.resources.service.dispose({ fatal: true }));
    await Promise.all([attempt(async () => { await this.resources.remote?.dispose(); }), attempt(async () => { await this.resources.keepAwake?.dispose(); })]);
    // Once the lock is lost, leaving SQLite open is unsafe even if a PTY refuses cleanup.
    await attempt(() => this.resources.closeRepository()); this.unsubscribeOwnership();
    await attempt(() => this.ownership.release());
    const fatal = new AggregateError(errors, cause.message + (errors.length > 1 ? ' Fatal shutdown could not confirm all cleanup.' : ' Host shut down.'));
    this.complete(fatal); throw fatal;
  }
}
export const startHostRuntime = (options: HostRuntimeOptions): Promise<HostRuntime> => HostRuntime.create(options);

/** Foreground Node host. The CLI owns argument parsing and service installation. */
export async function runHost(options: HostRuntimeOptions): Promise<void> {
  const runtime = await HostRuntime.create(options);
  await new Promise<void>((resolve, reject) => {
    const signals: NodeJS.Signals[] = process.platform === 'win32' ? ['SIGINT', 'SIGTERM', 'SIGBREAK'] : ['SIGINT', 'SIGTERM', 'SIGHUP'];
    let readyError: unknown;
    const cleanup = () => { for (const signal of signals) process.removeListener(signal, stop); };
    const stop = () => { void runtime.stop().catch(error => {
      if (!runtime.ownership.lost) console.error('Shellfox could not stop. The host is still running; inspect it and retry.', error);
      // Fatal loss completes through onStopped after waiting stop replies have flushed.
    }); };
    for (const signal of signals) process.on(signal, stop);
    runtime.onStopped(error => { cleanup(); if (readyError || error || runtime.ownership.lost) reject(readyError ?? error ?? runtime.ownership.lost); else resolve(); });
    try { options.onReady?.(runtime.status()); } catch (error) { readyError = error; void runtime.stop().catch(reject); }
  });
}
