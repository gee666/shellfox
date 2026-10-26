import type { CliIntegrationDto, ExplorerIntegrationDto, Result } from '../shared/contracts';
import { failure } from '../shared/contracts';
import { unavailableCli, unavailableExplorer } from './defaults';

interface State { supported: boolean; installed: boolean; reason: string | null }
interface Provider<T extends State> { get(): Promise<Result<T>>; set(installed: boolean): Promise<Result<T>>; readonly readOnly?: true }
type Kind = 'explorer' | 'cli';
interface Slot<T extends State> {
  value: T; provider?: Provider<T>; revision: number; mutations: number; checkedAt?: number;
  failed(previous: T, message: string): T;
}
const FRESHNESS_MS = 5000;
const message = (error: unknown) => error instanceof Error ? error.message : 'Integration check failed.';

/** Owns the always-on policy, repair freshness and provider write ordering. No preference storage. */
export class DesktopIntegrations {
  private readonly explorerSlot: Slot<ExplorerIntegrationDto>;
  private readonly cliSlot: Slot<CliIntegrationDto>;
  private readonly writes = new Map<string, Promise<void>>();
  private refreshing?: Promise<void>;
  private ready = false;
  constructor(explorer: Provider<ExplorerIntegrationDto> | undefined, cli: Provider<CliIntegrationDto> | undefined, private readonly active: () => boolean, private readonly changed: () => void, private readonly platform = process.platform) {
    this.explorerSlot = { value: { ...unavailableExplorer }, provider: explorer, revision: 0, mutations: 0,
      failed: (previous, reason) => ({ ...previous, installed: false, folderItemInstalled: false, backgroundInstalled: false, reason }) };
    this.cliSlot = { value: { ...unavailableCli }, provider: cli, revision: 0, mutations: 0, failed: (previous, reason) => ({ ...previous, installed: false, reason }) };
  }
  get explorer(): ExplorerIntegrationDto { return this.explorerSlot.value; }
  get cli(): CliIntegrationDto { return this.cliSlot.value; }
  markReady(): void { this.ready = true; this.explorerSlot.checkedAt = this.cliSlot.checkedAt = Date.now(); }
  async initialize(): Promise<void> {
    const ignoreChange = () => {};
    if (this.platform === 'linux') { await this.check(this.explorerSlot, 'explorer', ignoreChange); await this.check(this.cliSlot, 'cli', ignoreChange); }
    else {
      const work = [this.check(this.explorerSlot, 'explorer', ignoreChange), this.check(this.cliSlot, 'cli', ignoreChange)];
      await Promise.allSettled(work); await Promise.all(work);
    }
  }
  private needs<T extends State>(slot: Slot<T>): boolean {
    if (!slot.provider || slot.mutations) return false;
    const age = slot.checkedAt === undefined ? Infinity : Date.now() - slot.checkedAt;
    return age < 0 || age >= FRESHNESS_MS;
  }
  private valid<T extends State>(slot: Slot<T>, revision: number): boolean { return this.active() && slot.revision === revision && !slot.mutations; }
  private apply<T extends State>(slot: Slot<T>, result: Result<T>, changed: () => void, base = slot.value, fresh = true): void {
    const next = result.ok ? result.value : slot.failed(base, result.error.message);
    if (JSON.stringify(slot.value) !== JSON.stringify(next)) changed();
    slot.value = next; slot.checkedAt = fresh ? Date.now() : undefined;
  }
  private mutate(kind: Kind): () => void {
    // Linux file-manager menus also maintain the CLI launcher.
    const affected = this.platform === 'linux' ? [this.explorerSlot, this.cliSlot] : kind === 'explorer' ? [this.explorerSlot] : [this.cliSlot];
    for (const slot of affected) { slot.revision++; slot.mutations++; slot.checkedAt = undefined; }
    return () => { for (const slot of affected) { slot.revision++; slot.mutations--; if (slot !== this.slot(kind)) slot.checkedAt = undefined; } };
  }
  private slot(kind: Kind) { return kind === 'explorer' ? this.explorerSlot : this.cliSlot; }
  private enqueue<T>(kind: Kind, action: () => Promise<T>): Promise<T> {
    const key = this.platform === 'linux' ? 'shared' : kind;
    const task = (this.writes.get(key) ?? Promise.resolve()).then(action);
    this.writes.set(key, task.then(() => {}, () => {})); return task;
  }
  async drain(): Promise<void> { await Promise.all([...this.writes.values()]); }
  private async check<T extends State>(slot: Slot<T>, kind: Kind, changed: () => void): Promise<void> {
    const provider = slot.provider; if (!provider || !this.active()) return;
    const revision = slot.revision;
    let current: Result<T>;
    try { current = await provider.get(); }
    catch (error) {
      if (this.valid(slot, revision)) this.apply(slot, failure('NATIVE_UNAVAILABLE', message(error), true), changed, slot.value, false);
      throw error;
    }
    if (!this.valid(slot, revision)) return;
    if (!current.ok || !current.value.supported || current.value.installed || provider.readOnly) { this.apply(slot, current, changed); return; }
    const missing = current.value;
    await this.enqueue(kind, async () => {
      if (!this.valid(slot, revision)) return;
      const finish = this.mutate(kind), ownRevision = slot.revision;
      try {
        const repaired = await provider.set(true);
        if (this.active() && slot.revision === ownRevision && slot.mutations === 1) this.apply(slot, repaired, changed, missing);
      } catch (error) {
        if (this.active() && slot.revision === ownRevision && slot.mutations === 1) this.apply(slot, failure('NATIVE_UNAVAILABLE', message(error), true), changed, missing, false);
        throw error;
      } finally { finish(); }
    });
  }
  async refresh(): Promise<void> {
    if (!this.ready || !this.active()) return;
    if (this.refreshing) return this.refreshing;
    let changed = false;
    this.refreshing = (async () => {
      if (this.needs(this.explorerSlot)) await this.check(this.explorerSlot, 'explorer', () => { changed = true; });
      if (this.active() && this.needs(this.cliSlot)) await this.check(this.cliSlot, 'cli', () => { changed = true; });
    })().finally(() => { this.refreshing = undefined; if (changed && this.active()) this.changed(); });
    return this.refreshing;
  }
  private ensure<T extends State>(slot: Slot<T>, kind: Kind): Promise<Result<T>> {
    return this.enqueue(kind, async () => {
      const provider = slot.provider;
      if (!provider) return failure('UNSUPPORTED', 'This host has no desktop integration provider.');
      if (!this.active()) return failure('NATIVE_UNAVAILABLE', 'Shellfox is shutting down.');
      const finish = this.mutate(kind), revision = slot.revision; let changed = false;
      try {
        const current = await provider.get();
        const result = current.ok && current.value.supported && !current.value.installed && !provider.readOnly ? await provider.set(true) : current;
        if (this.active() && slot.revision === revision && slot.mutations === 1) this.apply(slot, result, () => { changed = true; }, current.ok ? current.value : slot.value);
        return result.ok && !result.value.supported && !provider.readOnly ? failure('UNSUPPORTED', result.value.reason ?? 'This integration is unsupported.') : result;
      } catch (error) {
        const result = failure('NATIVE_UNAVAILABLE', message(error), true);
        if (this.active() && slot.revision === revision && slot.mutations === 1) this.apply(slot, result, () => { changed = true; }, slot.value, false);
        return result;
      } finally { finish(); if (changed && this.active()) this.changed(); }
    });
  }
  ensureExplorer(): Promise<Result<ExplorerIntegrationDto>> { return this.ensure(this.explorerSlot, 'explorer'); }
  ensureCli(): Promise<Result<CliIntegrationDto>> { return this.ensure(this.cliSlot, 'cli'); }
}
