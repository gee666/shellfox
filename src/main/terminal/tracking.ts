import { execFile } from 'node:child_process';
import { open, readdir, readlink, access } from 'node:fs/promises';
import { hostname } from 'node:os';
import { constants } from 'node:fs';
import { win32, join, resolve } from 'node:path';
import type { ProcessRule } from '../../shared/contracts';
import type { TabObservation } from '../../shared/native-port';

export interface TrackingRoot {
  tabId: string;
  sessionId: string;
  generation: string;
  pid: number;
  environment: 'local' | 'wsl';
  distro: string | null;
  marker: string;
  /** Canonical launched shell, not the WSL host transport. Basename must stay unchanged. */
  shellExecutable?: string;
  /** Exact shell birth authenticated over the private native supervisor channel.
   * System-shell environments may be redacted; this evidence is backend-only. */
  authenticatedBirth?: string;
  /** Trusted manager PID in the local host domain, used only for initial ancestry proof. */
  ancestorPid?: number;
  /** Inclusive bounds in the provider's exact decimal birthOrder units, not a birth identity. */
  birthOrderBounds?: { min: string; max: string };
}

/** Birth is opaque, exact evidence from the OS, not a rounded Date or a PID.
 * birthOrder orders births within this snapshot's clock/boot. Without it, new
 * parent links cannot be verified. Missing birth evidence is null, never synthesized. */
export interface TrackingProcess {
  pid: number;
  parentPid: number;
  birth: string | null;
  birthOrder?: string;
  executable: string | null;
  argv: string[] | null;
  /** OS process name/comm, when the collector exposes it. Status matching only. */
  processName?: string | null;
  accessible: boolean;
  marker?: string | null;
  /** Native metadata is informational, not authority to signal a numeric PID. */
  pgid?: number | null;
  sid?: number | null;
  uid?: number | null;
  realUid?: number | null;
  savedUid?: number | null;
}
export interface TrackingSnapshot {
  /** Stable host + process namespace, including the guest distro for WSL. */
  domain: string;
  environment: 'local' | 'wsl';
  distro: string | null;
  processes: TrackingProcess[];
  /** False if enumeration itself missed evidence, not merely an unrelated argv. */
  complete: boolean;
  reason?: string;
  /** The provider can authenticate initial local roots through their injected env. */
  rootMarkerRequired?: boolean;
  /** Windows cannot read the root marker through CIM. Require launch-window/ancestry proof. */
  rootLaunchEvidenceRequired?: boolean;
  /** Optional guest enumeration for inherited-marker status, never root/owner evidence. */
  statusOnly?: boolean;
}
export type SnapshotProvider = (
  roots: readonly TrackingRoot[], signal?: AbortSignal,
) => Promise<TrackingSnapshot[]>;

const MAX_ROWS = 20_000;
const MAX_OUTPUT = 8 * 1024 * 1024;
const MAX_FIELD = 64 * 1024;
const SNAPSHOT_TIMEOUT = 8_000;
const key = (domain: string, p: TrackingProcess) => JSON.stringify([domain, p.pid, p.birth]);
const watchKey = (r: TrackingRoot) => JSON.stringify([
  r.tabId, r.sessionId, r.generation, r.environment, r.distro, r.pid, r.marker,
  r.shellExecutable, r.authenticatedBirth, r.ancestorPid, r.birthOrderBounds?.min, r.birthOrderBounds?.max,
]);
const basename = (p: string, windows = false) => (windows ? p.replace(/\\/g, '/') : p).split('/').pop()!;
const shellNames = new Set(['sh', 'bash', 'dash', 'ash', 'zsh', 'fish', 'ksh', 'csh', 'tcsh', 'nu', 'pwsh', 'powershell']);
const shellBasename = (executable: string, windows: boolean) => windows
  ? basename(executable, true).toLowerCase() : basename(executable);
const isShell = (p: TrackingProcess, windows = false) => p.executable !== null &&
  shellNames.has(windows ? shellBasename(p.executable, true).replace(/\.exe$/, '') : basename(p.executable));
const ordered = (p: TrackingProcess, parent: TrackingProcess) =>
  p.birthOrder !== undefined && parent.birthOrder !== undefined && BigInt(parent.birthOrder) <= BigInt(p.birthOrder);

const nodeValues = new Set([
  '-r', '--require', '--import', '--loader', '--experimental-loader', '--title', '--conditions', '-C',
  '--input-type', '--unhandled-rejections', '--redirect-warnings', '--trace-event-categories',
  '--trace-event-file-pattern', '--icu-data-dir', '--openssl-config', '--heap-prof-dir', '--heap-prof-name',
  '--cpu-prof-dir', '--cpu-prof-name', '--test-name-pattern', '--test-reporter', '--test-reporter-destination',
  '--test-concurrency', '--test-shard', '--disable-warning', '--inspect-port', '--heapsnapshot-signal',
  '--heapsnapshot-near-heap-limit', '--snapshot-blob',
]);
const nodeFlags = new Set([
  '--no-warnings', '--enable-source-maps', '--trace-warnings', '--trace-uncaught', '--trace-deprecation',
  '--no-deprecation', '--throw-deprecation', '--use-strict', '--experimental-modules',
  '--inspect', '--inspect-brk', '--inspect-wait', '--test',
]);
interface ScriptSlot { path: string | null; unsupported: boolean }
function scriptSlot(executable: string, argv: string[], windows: boolean): ScriptSlot {
  const none = { path: null, unsupported: false };
  const unknown = { path: null, unsupported: true };
  const exe = windows ? basename(executable, true).toLowerCase().replace(/\.exe$/, '') : basename(executable);
  if (exe === 'pwsh' || exe === 'powershell') {
    for (let i = 1; i < argv.length; i++) {
      const a = argv[i]!.toLowerCase();
      if (['-command', '-c', '-encodedcommand', '-enc', '-ec', '-e', '-commandwithargs'].includes(a)) return none;
      if (a === '-file' || a === '-f') return argv[i + 1] ? { path: argv[i + 1]!, unsupported: false } : unknown;
      if (['-executionpolicy', '-ep', '-workingdirectory', '-wd', '-inputformat', '-outputformat', '-windowstyle', '-configurationname'].includes(a)) {
        if (++i >= argv.length) return unknown;
      } else if (!['-nologo', '-noprofile', '-nop', '-noexit', '-noninteractive'].includes(a)) return unknown;
    }
    return none;
  }
  if (!['node', 'nodejs', 'bun'].includes(exe)) return unknown;
  for (let i = 1; i < argv.length; i++) {
    const a = argv[i]!;
    if (a === '--') return { path: argv[i + 1] ?? null, unsupported: false };
    if (['-e', '--eval', '-p', '--print'].includes(a) || /^(--eval=|--print=|-[ep].)/.test(a)) return none;
    if (nodeValues.has(a)) { if (++i >= argv.length) return unknown; continue; }
    if (nodeFlags.has(a)) continue;
    // Only known value options accept '='. An unknown flag could change the script slot.
    if (a.includes('=') && (nodeValues.has(a.split('=')[0]!) || ['--inspect', '--inspect-brk', '--inspect-wait'].includes(a.split('=')[0]!))) continue;
    if (exe === 'bun' && a === 'run') {
      const path = argv[i + 1];
      // Package.json script names and other Bun subcommands are not launch paths.
      return path && !path.startsWith('-') && (/[\\/]/.test(path) || /\.[cm]?[jt]sx?$/.test(path))
        ? { path, unsupported: false } : none;
    }
    if (exe === 'bun' && ['x', 'exec', 'test', 'install', 'build', 'add', 'remove', 'update'].includes(a)) return none;
    if (a.startsWith('-')) return unknown;
    return { path: a, unsupported: false };
  }
  return none;
}
function executableMatches(rule: ProcessRule, executable: string, windows: boolean): boolean {
  const normalize = (p: string) => windows ? win32.normalize(p).toLowerCase() : p;
  return rule.executablePaths.length > 0
    ? rule.executablePaths.some(p => normalize(p) === normalize(executable))
    : rule.executableBasenames.some(p => windows ? p.toLowerCase() === basename(executable, true).toLowerCase() : p === basename(executable));
}
function suffixMatches(path: string, suffix: string, windows: boolean): boolean {
  const normalize = (s: string) => {
    const v = (windows ? win32.normalize(s).replace(/\\/g, '/') : s).replace(/^\/+/, '');
    return windows ? v.toLowerCase() : v;
  };
  const p = normalize(path), s = normalize(suffix);
  return p === s || p.endsWith('/' + s);
}

function snapshotRows(s: TrackingSnapshot): Map<number, TrackingProcess> {
  if (typeof s.domain !== 'string' || !s.domain || typeof s.complete !== 'boolean' || !Array.isArray(s.processes) || s.processes.length > MAX_ROWS) throw new Error('Invalid snapshot.');
  const rows = new Map<number, TrackingProcess>();
  for (const p of s.processes) {
    if (!Number.isSafeInteger(p.pid) || p.pid <= 0 || !Number.isSafeInteger(p.parentPid) || p.parentPid < 0 || rows.has(p.pid) ||
      p.birthOrder !== undefined && (typeof p.birthOrder !== 'string' || !/^\d+$/.test(p.birthOrder)) ||
      p.birth !== null && (typeof p.birth !== 'string' || !p.birth) || typeof p.accessible !== 'boolean' ||
      p.executable !== null && typeof p.executable !== 'string' ||
      p.argv !== null && (!Array.isArray(p.argv) || p.argv.some(a => typeof a !== 'string')) ||
      p.processName != null && (typeof p.processName !== 'string' || p.processName.includes('\0') ||
        Buffer.byteLength(p.processName) > MAX_FIELD)) throw new Error('Invalid process identity.');
    rows.set(p.pid, p);
  }
  return rows;
}

function matchAgent(p: TrackingProcess, rules: ProcessRule[], windows: boolean): { matched: boolean; reason: string | null } {
  let reason: string | null = null;
  if (!p.executable) return { matched: false, reason };
  const equalName = (a: string, b: string) => windows ? a.toLowerCase() === b.toLowerCase() : a === b;
  const names = [basename(p.executable, windows), p.processName, p.argv?.[0] && basename(p.argv[0], windows)];
  const named = rules.filter(rule => {
    if (rule.executablePaths.length) return false;
    const trusted = rule.processNames ?? (rule.scriptPathSuffixes.length ? [] : rule.executableBasenames);
    return trusted.some(name => names.some(actual => actual != null && equalName(name, actual)));
  });
  if (named.some(rule => rule.enabled)) return { matched: true, reason: null };
  for (const rule of rules) {
    if (!rule.enabled) continue;
    // A recognized disabled name cannot fall through to another agent's script.
    // Exact-path custom rules remain independent and never use name/title evidence.
    if (named.length && !rule.executablePaths.length) continue;
    if (!executableMatches(rule, p.executable, windows)) continue;
    if (!rule.scriptPathSuffixes.length) return { matched: true, reason: null };
    if (!p.argv) { reason = 'A relevant descendant command line is inaccessible.'; continue; }
    const script = scriptSlot(p.executable, p.argv, windows);
    if (script.unsupported) reason = 'A script rule uses an unsupported interpreter or option form.';
    if (script.path && rule.scriptPathSuffixes.some(suffix => suffixMatches(script.path!, suffix, windows))) {
      return { matched: true, reason: null };
    }
  }
  return { matched: false, reason };
}

/** No native helper, persistent child, or command launch until there is a watch. */
export class ProcessTracker {
  private roots: TrackingRoot[] = [];
  private rules: ProcessRule[] = [];
  private revision = 0;
  private disposed = false;
  private timer: ReturnType<typeof setInterval> | undefined;
  private pending: Promise<void> | undefined;
  private readonly abort = new AbortController();
  private readonly identities = new Map<string, string>();
  private readonly rootExecutables = new Map<string, string>();
  private readonly owners = new Map<string, string>();
  private readonly parentLinks = new Map<string, { pid: number; birth: string }>();

  constructor(
    private readonly onObservations: (items: TabObservation[]) => void,
    private readonly snapshotProvider: SnapshotProvider = createSystemSnapshotProvider(),
  ) {}

  setWatch(roots: TrackingRoot[], rules: ProcessRule[]): void {
    if (this.disposed) return;
    if (roots.length > 1000 || rules.length > 100 || new Set(roots.map(r => r.tabId)).size !== roots.length ||
      roots.some(r => !Number.isSafeInteger(r.pid) || r.pid <= 0 ||
        r.environment !== 'local' && r.environment !== 'wsl' ||
        r.environment === 'local' && r.distro !== null || r.environment === 'wsl' && (!r.distro || !r.marker) ||
        r.shellExecutable !== undefined && (typeof r.shellExecutable !== 'string' || !r.shellExecutable || r.shellExecutable.includes('\0')) ||
        r.authenticatedBirth !== undefined && (r.environment !== 'local' || !/^darwin:\d{1,20}:\d{6}$/.test(r.authenticatedBirth)) ||
        r.ancestorPid !== undefined && (r.environment !== 'local' || !Number.isSafeInteger(r.ancestorPid) || r.ancestorPid <= 0 || r.ancestorPid === r.pid) ||
        r.birthOrderBounds !== undefined && (!/^\d{1,80}$/.test(r.birthOrderBounds.min) || !/^\d{1,80}$/.test(r.birthOrderBounds.max) ||
          BigInt(r.birthOrderBounds.min) > BigInt(r.birthOrderBounds.max)))) {
      throw new Error('Invalid tracking watch.');
    }
    this.roots = roots.map(r => ({ ...r, birthOrderBounds: r.birthOrderBounds ? { ...r.birthOrderBounds } : undefined }));
    this.rules = rules.map(r => ({ ...r, executablePaths: [...r.executablePaths], executableBasenames: [...r.executableBasenames], scriptPathSuffixes: [...r.scriptPathSuffixes] }));
    const retained = new Set(this.roots.map(watchKey));
    for (const id of this.identities.keys()) if (!retained.has(id)) { this.identities.delete(id); this.rootExecutables.delete(id); }
    for (const [id, owner] of this.owners) if (!retained.has(owner)) this.owners.delete(id);
    this.revision++;
    if (roots.length && !this.timer) {
      this.timer = setInterval(() => { void this.poll().catch(() => {}); }, 1000);
      this.timer.unref?.();
    } else if (!roots.length && this.timer) {
      clearInterval(this.timer);
      this.timer = undefined;
      this.parentLinks.clear();
    }
    void this.poll().catch(() => {});
  }

  resolveIdentity(root: TrackingRoot): { domain: string; pid: number; birth: string } | null {
    const pinned = this.identities.get(watchKey(root));
    if (!pinned) return null;
    const [domain, pid, birth] = JSON.parse(pinned) as [string, number, string];
    return { domain, pid, birth };
  }

  /** Last proven descendants, including retained/reparented owners. This is NOT
   * fresh kill authority: pidfd callers must revalidate domain + exact birth.
   * The interactive root and other watched interactive roots are excluded. */
  resolveDescendants(root: TrackingRoot): Array<{ domain: string; pid: number; birth: string }> {
    const owner = watchKey(root);
    if (this.disposed || !this.identities.has(owner)) return [];
    const interactive = new Set(this.identities.values());
    const descendants: Array<{ domain: string; pid: number; birth: string }> = [];
    for (const [identity, assigned] of this.owners) {
      if (assigned !== owner || interactive.has(identity)) continue;
      const [domain, pid, birth] = JSON.parse(identity) as [string, number, string];
      descendants.push({ domain, pid, birth });
    }
    return descendants;
  }

  dispose(): void {
    this.disposed = true;
    this.abort.abort();
    if (this.timer) clearInterval(this.timer);
    this.timer = undefined;
    this.roots = [];
    this.identities.clear();
    this.rootExecutables.clear();
    this.owners.clear();
    this.parentLinks.clear();
  }

  poll(): Promise<void> {
    if (this.disposed) return Promise.resolve();
    if (this.pending) return this.pending;
    this.pending = this.pollCurrent().finally(() => { this.pending = undefined; });
    return this.pending;
  }

  private async pollCurrent(): Promise<void> {
    let revision: number;
    do {
      revision = this.revision;
      const roots = this.roots;
      let snapshots: TrackingSnapshot[] = [];
      try {
        if (roots.length) snapshots = await this.snapshotProvider(roots, this.abort.signal);
      } catch { /* Provider failures cannot establish waiting or exit. */ }
      if (this.disposed) return;
      if (revision !== this.revision) continue;
      const observedAt = new Date().toISOString();
      const items = new Map<string, TabObservation>();
      for (const r of roots) items.set(watchKey(r), {
        sessionId: r.sessionId, tabId: r.tabId, observedAt, root: 'unavailable',
        health: 'unknown', agents: 0, reason: 'Process snapshot unavailable.',
      });
      const groups = new Map<string, TrackingRoot[]>();
      for (const r of roots) {
        const matches = snapshots.filter(s => !s.statusOnly && s.environment === r.environment && s.distro === r.distro);
        if (matches.length !== 1) continue;
        const s = matches[0]!;
        if (snapshots.filter(other => other.domain === s.domain).length !== 1) continue;
        const group = groups.get(s.domain) ?? [];
        group.push(r);
        groups.set(s.domain, group);
      }
      for (const [domain, watched] of groups) {
        const s = snapshots.find(s => s.domain === domain)!;
        try { this.observe(s, watched, observedAt).forEach(item => items.set(watchKey(watched.find(r => r.tabId === item.tabId)!), item)); }
        catch { /* Malformed or ambiguous snapshot evidence remains unknown. */ }
      }
      // Marker association is status only. Do not resolve ancestry or retain guest owners.
      if (process.platform === 'win32') {
        const marked = new Map(roots.filter(r => r.marker && roots.filter(other => other.marker === r.marker).length === 1)
          .map(r => [r.marker, items.get(watchKey(r))!]));
        for (const s of snapshots.filter(s => s.environment === 'wsl')) {
          if (snapshots.filter(other => other.domain === s.domain).length !== 1) continue;
          try {
            for (const p of snapshotRows(s).values()) {
              const item = p.marker ? marked.get(p.marker) : undefined;
              if (!item || item.root !== 'alive' || this.owners.has(key(s.domain, p))) continue;
              if (!s.complete) { item.reason ??= s.reason ?? 'Marked guest process enumeration was incomplete.'; item.health = 'unknown'; }
              const match = p.accessible && p.birth !== null && p.executable
                ? matchAgent(p, this.rules, false) : { matched: false, reason: 'A marked guest process is inaccessible.' };
              if (match.matched) item.agents++;
              if (match.reason) { item.reason ??= match.reason; item.health = 'unknown'; }
            }
          } catch { /* Optional malformed/unlinked guest evidence does not change root health. */ }
        }
      }
      this.onObservations(roots.map(r => items.get(watchKey(r))!));
    } while (!this.disposed && revision !== this.revision);
  }

  private observe(s: TrackingSnapshot, watched: TrackingRoot[], observedAt: string): TabObservation[] {
    const rows = snapshotRows(s);
    const alive = new Set(s.processes.filter(p => p.birth !== null).map(p => key(s.domain, p)));
    const rootRows = new Map<string, TrackingProcess>();
    const rootOwners = new Map<string, string>();
    const rootErrors = new Map<string, string>();
    const windows = s.environment === 'local' && process.platform === 'win32';
    const hasAncestor = (p: TrackingProcess, expectedPid: number): boolean => {
      const seen = new Set<number>([p.pid]);
      let current = p;
      while (true) {
        const parent = rows.get(current.parentPid);
        if (!parent || parent.birth === null || seen.has(parent.pid) || !ordered(current, parent)) return false;
        // CIM verifies each ancestry row's PID/birth twice. Token reads intentionally
        // omit unrelated ancestors; their accessible flag is not ancestry evidence.
        if (parent.pid === expectedPid) return true;
        seen.add(parent.pid); current = parent;
      }
    };
    for (const r of watched) {
      const id = watchKey(r), pinned = this.identities.get(id);
      let p: TrackingProcess | undefined;
      if (pinned) p = s.processes.find(p => p.birth !== null && key(s.domain, p) === pinned);
      else if (r.environment === 'local') p = rows.get(r.pid);
      else if (s.complete && !s.processes.some(p => p.marker === r.marker && (!p.accessible || p.birth === null || !p.executable))) {
        const candidates = s.processes.filter(p => p.marker === r.marker && p.accessible && p.birth !== null);
        // Select the oldest marked process first. If the actual shell exec'd Node,
        // a younger marked shell must not be substituted for that root.
        const top = candidates.filter(p => {
          const seen = new Set<number>([p.pid]);
          let parent = rows.get(p.parentPid);
          while (parent && !seen.has(parent.pid) && ordered(p, parent)) {
            if (candidates.includes(parent)) return false;
            seen.add(parent.pid);
            parent = rows.get(parent.parentPid);
          }
          return true;
        });
        if (top.length === 1) p = top[0];
        else if (top.length > 1 && top.every(p => p.birthOrder !== undefined)) {
          const sorted = [...top].sort((a, b) => BigInt(a.birthOrder!) < BigInt(b.birthOrder!) ? -1 : BigInt(a.birthOrder!) > BigInt(b.birthOrder!) ? 1 : 0);
          if (sorted[0]!.birthOrder !== sorted[1]!.birthOrder) p = sorted[0];
        }
      }
      if (!pinned && r.authenticatedBirth !== undefined && p?.birth !== r.authenticatedBirth) {
        rootErrors.set(id, 'Owned shell birth does not match its authenticated supervisor identity.');
        continue;
      }
      if (!pinned && r.environment === 'local' && p && r.authenticatedBirth === undefined &&
        (s.rootMarkerRequired || p.marker !== undefined) && (!r.marker || p.marker !== r.marker)) {
        rootErrors.set(id, 'Owned shell environment marker could not be verified.');
        continue;
      }
      if (!p || p.birth === null || !p.accessible || !p.executable) {
        rootErrors.set(id, s.reason ?? 'Owned shell identity or access could not be verified.');
        continue;
      }
      const executable = shellBasename(p.executable, windows), expected = r.shellExecutable
        ? shellBasename(r.shellExecutable, windows) : this.rootExecutables.get(id);
      if (expected ? executable !== expected : !isShell(p, windows)) {
        rootErrors.set(id, 'Owned root is not the launched interactive shell; its executable changed or is unverified.');
        continue;
      }
      if (!pinned) {
        const bounds = r.birthOrderBounds;
        const bounded = bounds && p.birthOrder !== undefined && BigInt(p.birthOrder) >= BigInt(bounds.min) && BigInt(p.birthOrder) <= BigInt(bounds.max);
        if (s.rootLaunchEvidenceRequired && (!r.shellExecutable || !bounds || r.ancestorPid === undefined) ||
          bounds && !bounded || r.ancestorPid !== undefined && !hasAncestor(p, r.ancestorPid)) {
          rootErrors.set(id, 'Initial shell birth window or manager ancestry could not be verified.');
          continue;
        }
      }
      const identity = key(s.domain, p);
      if (rootOwners.has(identity)) {
        rootErrors.set(id, 'Multiple tabs claim the same process identity.');
        rootErrors.set(rootOwners.get(identity)!, 'Multiple tabs claim the same process identity.');
      }
      this.identities.set(id, identity);
      this.rootExecutables.set(id, executable);
      rootRows.set(id, p);
      rootOwners.set(identity, id);
    }
    const retained = new Set(this.roots.map(watchKey));
    for (const [identity, owner] of this.owners) {
      // A failed/partial enumeration is not proof that a descendant exited.
      if (!retained.has(owner) || s.complete && identity.startsWith(JSON.stringify([s.domain]).slice(0, -1) + ',') && !alive.has(identity)) this.owners.delete(identity);
    }
    for (const identity of this.parentLinks.keys()) {
      if (s.complete && identity.startsWith(JSON.stringify([s.domain]).slice(0, -1) + ',') && !alive.has(identity)) this.parentLinks.delete(identity);
    }
    for (const [identity, owner] of rootOwners) this.owners.set(identity, owner);
    const resolve = (p: TrackingProcess): string | undefined => {
      const trail: TrackingProcess[] = [];
      const seen = new Set<number>();
      let current: TrackingProcess | undefined = p, owner: string | undefined;
      while (current && !seen.has(current.pid)) {
        seen.add(current.pid);
        if (current.birth === null || !current.accessible) break;
        const identity = key(s.domain, current);
        owner = rootOwners.get(identity) ?? this.owners.get(identity);
        if (owner) break;
        trail.push(current);
        const parent = rows.get(current.parentPid);
        if (!parent || parent.birth === null || !ordered(current, parent)) break;
        const priorParent = this.parentLinks.get(identity);
        // Keep the first exact parent generation for an unchanged numeric link.
        // Updating it after just one poll would silently adopt the child on the next poll.
        if (priorParent?.pid === parent.pid && priorParent.birth !== parent.birth) break;
        this.parentLinks.set(identity, { pid: parent.pid, birth: parent.birth });
        current = parent;
      }
      if (owner) for (const p of trail) this.owners.set(key(s.domain, p), owner);
      return owner;
    };
    for (const p of s.processes) resolve(p);
    return watched.map(r => {
      const id = watchKey(r), rootRow = rootRows.get(id), pinned = this.identities.get(id);
      let reason: string | null = rootErrors.get(id) ?? (s.complete ? null : s.reason ?? 'Process enumeration was incomplete.');
      let root: TabObservation['root'] = rootRow ? 'alive' : 'unavailable';
      // An unreadable row is not proof of exit. A changed host/domain is not either.
      const pinnedParts = pinned ? JSON.parse(pinned) as [string, number, string] : undefined;
      const samePid = pinnedParts ? rows.get(pinnedParts[1]) : undefined;
      if (!rootRow && pinnedParts && pinnedParts[0] === s.domain && s.complete && !alive.has(pinned!) && (!samePid || samePid.birth !== null)) {
        root = 'exited';
        reason = null;
      } else if (!rootRow && !pinned && r.environment === 'local' && s.complete && !rows.has(r.pid)) {
        root = 'exited';
        reason = null;
      }
      let agents = 0;
      for (const p of s.processes) {
        const owned = p.birth !== null && this.owners.get(key(s.domain, p)) === id;
        if (rootOwners.has(key(s.domain, p))) continue;
        const parent = rows.get(p.parentPid);
        if (!owned) {
          if (parent?.birth && this.owners.get(key(s.domain, parent)) === id) {
            if (p.birthOrder === undefined || parent.birthOrder === undefined) reason = 'Descendant birth ordering is unavailable.';
            else if (!ordered(p, parent)) reason = 'Descendant birth order cannot authenticate ancestry.';
            else if (!p.accessible || p.birth === null) reason = 'A descendant is inaccessible.';
          }
          continue;
        }
        if (!p.accessible || !p.executable) { reason = 'A descendant is inaccessible.'; continue; }
        const match = matchAgent(p, this.rules, windows);
        if (match.reason) reason = match.reason;
        if (match.matched) agents++;
      }
      return { sessionId: r.sessionId, tabId: r.tabId, observedAt, root, health: root === 'alive' && reason === null ? 'healthy' : 'unknown', agents: root === 'alive' ? agents : 0, reason };
    });
  }
}

function command(executable: string, args: string[], signal?: AbortSignal, env?: NodeJS.ProcessEnv, wslList = false): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(executable, args, { timeout: SNAPSHOT_TIMEOUT, maxBuffer: MAX_OUTPUT, encoding: wslList ? 'buffer' : 'utf8', windowsHide: true, signal, env },
      (error, stdout) => error ? reject(error) : resolve(Buffer.isBuffer(stdout) ? stdout.toString(stdout.includes(0) ? 'utf16le' : 'utf8') : stdout));
  });
}
async function windowsExecutable(name: 'powershell' | 'wsl'): Promise<string> {
  const windows = process.env.SystemRoot ?? process.env.WINDIR ?? 'C:\\Windows';
  for (const system of process.arch === 'ia32' ? ['Sysnative', 'System32'] : ['System32']) {
    const path = name === 'wsl' ? win32.join(windows, system, 'wsl.exe') : win32.join(windows, system, 'WindowsPowerShell', 'v1.0', 'powershell.exe');
    try { await access(path); return path; } catch { /* Try the next OS location, never a renderer-supplied executable. */ }
  }
  throw new Error('Windows snapshot executable unavailable.');
}

// Two CIM enumerations reject PID reuse while owner evidence is being read.
const WINDOWS_SNAPSHOT = String.raw`
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$me = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
# CIM GetOwnerSid is a separate RPC per descendant. Under multi-tab load that
# exceeds the whole-command deadline. Read the same token-user SID locally.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Security.Principal;
public static class ShellfoxProcessOwner {
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
  [DllImport("advapi32.dll", SetLastError=true)] static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
  public static string Sid(uint pid) {
    IntPtr process = OpenProcess(0x1000, false, pid), token = IntPtr.Zero;
    if (process == IntPtr.Zero) return null;
    try {
      if (!OpenProcessToken(process, 8, out token)) return null;
      using (var identity = new WindowsIdentity(token)) { return identity.User.Value; }
    } catch { return null; }
    finally { if (token != IntPtr.Zero) CloseHandle(token); CloseHandle(process); }
  }
}
'@
$first = @(Get-CimInstance Win32_Process -OperationTimeoutSec 5)
if ($first.Count -gt 20000) { throw 'Process limit' }
$watch = ConvertFrom-Json $env:SHELLFOX_TRACKING_SNAPSHOT
$byPid = @{}; $children = @{}
foreach ($p in $first) {
  $byPid[[int]$p.ProcessId] = $p
  $parent = [int]$p.ParentProcessId
  if (!$children.ContainsKey($parent)) { $children[$parent] = New-Object 'System.Collections.Generic.List[int]' }
  $children[$parent].Add([int]$p.ProcessId)
}
$relevant = New-Object 'System.Collections.Generic.HashSet[int]'
$queue = New-Object 'System.Collections.Generic.Queue[int]'
foreach ($id in $watch.roots) { $queue.Enqueue([int]$id) }
foreach ($known in $watch.known) {
  $p = $byPid[[int]$known.pid]
  if ($p -and $p.CreationDate -and $p.CreationDate.ToUniversalTime().ToFileTimeUtc().ToString([Globalization.CultureInfo]::InvariantCulture) -eq $known.birth) { $queue.Enqueue([int]$known.pid) }
}
while ($queue.Count -gt 0) {
  $id = $queue.Dequeue()
  if ($relevant.Add($id)) { foreach ($child in $children[$id]) { $queue.Enqueue($child) } }
}
$rows = @($first | ForEach-Object {
  $p = $_; $ok = $false
  $birth = $null
  if ($p.CreationDate) { $birth = $p.CreationDate.ToUniversalTime().ToFileTimeUtc().ToString([Globalization.CultureInfo]::InvariantCulture) }
  if ($relevant.Contains([int]$p.ProcessId)) {
    $ok = ([ShellfoxProcessOwner]::Sid([uint32]$p.ProcessId) -eq $me)
  }
  [pscustomobject]@{ pid=[int]$p.ProcessId; parentPid=[int]$p.ParentProcessId; birth=$birth; birthOrder=$birth; executable=$p.ExecutablePath; processName=$p.Name; commandLine=$p.CommandLine; accessible=($ok -and $null -ne $birth -and $null -ne $p.ExecutablePath) }
})
$after = @{}
Get-CimInstance Win32_Process -OperationTimeoutSec 5 | ForEach-Object { $after[[int]$_.ProcessId] = $_ }
foreach ($r in $rows) {
  $p = $after[$r.pid]
  if (!$p -or !$p.CreationDate -or $p.CreationDate.ToUniversalTime().ToFileTimeUtc().ToString([Globalization.CultureInfo]::InvariantCulture) -ne $r.birth -or [int]$p.ParentProcessId -ne $r.parentPid) {
    $r.birth = $null; $r.birthOrder = $null; $r.accessible = $false
  }
}
ConvertTo-Json -InputObject $rows -Depth 4 -Compress
`;

// Windows command-line quoting, not shell tokenization. Unbalanced quotes are unknown.
function windowsArgs(line: string): string[] | null {
  if (line.length > MAX_FIELD) return null;
  const args: string[] = [];
  let i = 0;
  while (i < line.length) {
    while (/\s/.test(line[i] ?? '') && i < line.length) i++;
    if (i === line.length) break;
    let value = '', quoted = false;
    while (i < line.length && (quoted || !/\s/.test(line[i]!))) {
      let slashes = 0;
      while (line[i] === '\\') { slashes++; i++; }
      if (line[i] === '"') {
        value += '\\'.repeat(Math.floor(slashes / 2));
        if (slashes % 2) value += '"';
        else if (quoted && line[i + 1] === '"') { value += '"'; i++; }
        else quoted = !quoted;
        i++;
      } else {
        value += '\\'.repeat(slashes);
        if (i < line.length) value += line[i++]!;
      }
    }
    if (quoted) return null;
    args.push(value);
  }
  return args;
}
async function windowsSnapshot(roots: readonly TrackingRoot[], known: Map<number, string>, signal?: AbortSignal): Promise<TrackingProcess[]> {
  const scope = JSON.stringify({ roots: roots.map(r => r.pid), known: [...known].map(([pid, birth]) => ({ pid, birth })) });
  if (scope.length > 30_000) throw new Error('Windows snapshot scope exceeds environment bounds.');
  const output = await command(await windowsExecutable('powershell'), ['-NoLogo', '-NoProfile', '-NonInteractive', '-Command', WINDOWS_SNAPSHOT], signal,
    { ...process.env, SHELLFOX_TRACKING_SNAPSHOT: scope });
  const data: unknown = JSON.parse(output.replace(/^\uFEFF/, ''));
  if (!Array.isArray(data) || data.length > MAX_ROWS) throw new Error('Invalid CIM snapshot.');
  const processes = data.map(p => {
    if (!p || typeof p !== 'object') throw new Error('Invalid CIM process.');
    const row = p as Record<string, unknown>;
    return {
      pid: Number(row.pid), parentPid: Number(row.parentPid), birth: typeof row.birth === 'string' ? row.birth : null,
      birthOrder: typeof row.birthOrder === 'string' ? row.birthOrder : undefined,
      executable: typeof row.executable === 'string' ? row.executable : null,
      processName: typeof row.processName === 'string' ? row.processName : null,
      argv: typeof row.commandLine === 'string' ? windowsArgs(row.commandLine) : null,
      accessible: row.accessible === true,
    };
  });
  // CIM includes System Idle Process (PID 0). It cannot be an owned root/ancestor,
  // and passing it to the positive-PID tracker invalidates every otherwise valid row.
  const ownedProcesses = processes.filter(p => p.pid !== 0);
  const alive = new Set(ownedProcesses.map(p => p.pid));
  for (const pid of known.keys()) if (!alive.has(pid)) known.delete(pid);
  for (const p of processes) {
    if (p.birth && p.accessible) known.set(p.pid, p.birth);
    else if (p.birth && known.get(p.pid) !== p.birth) known.delete(p.pid);
  }
  return ownedProcesses;
}

async function boundedRead(path: string, signal?: AbortSignal): Promise<string> {
  const file = await open(path, 'r');
  try {
    const buffer = Buffer.alloc(MAX_FIELD + 1);
    let size = 0;
    while (size < buffer.length) {
      signal?.throwIfAborted();
      const result = await file.read(buffer, size, buffer.length - size, null);
      if (!result.bytesRead) return buffer.subarray(0, size).toString('utf8');
      size += result.bytesRead;
    }
    throw new Error('Process field limit exceeded.');
  } finally { await file.close(); }
}
function procStat(text: string): { parentPid: number; ticks: string; dead: boolean; name: string } {
  const end = text.lastIndexOf(')');
  const fields = text.slice(end + 2).trim().split(/\s+/);
  if (end < 0 || !/^\d+$/.test(fields[19] ?? '') || !/^\d+$/.test(fields[1] ?? '')) throw new Error('Invalid proc stat.');
  return { parentPid: Number(fields[1]), ticks: fields[19]!, dead: ['Z', 'X', 'x'].includes(fields[0]!), name: text.slice(text.indexOf('(') + 1, end) };
}
function environmentMarker(environ: string): string | null {
  const prefix = 'SHELLFOX_TERMINAL_MARKER=';
  const values = environ.split('\0').filter(e => e.startsWith(prefix));
  return values.length === 1 ? values[0]!.slice(prefix.length) : null;
}
async function linuxSnapshot(roots: readonly TrackingRoot[], signal?: AbortSignal): Promise<{ processes: TrackingProcess[]; complete: boolean; rootMarkerRequired: true }> {
  const rootPids = new Set(roots.map(r => r.pid));
  const uid = process.getuid?.();
  if (uid === undefined || uid !== process.geteuid?.()) throw new Error('Same-user inspection unavailable.');
  const boot = (await boundedRead('/proc/sys/kernel/random/boot_id', signal)).trim();
  if (!/^[a-f\d-]{36}$/i.test(boot)) throw new Error('Boot identity unavailable.');
  const pids = (await readdir('/proc')).filter(p => /^\d+$/.test(p));
  if (pids.length > MAX_ROWS) throw new Error('Process limit exceeded.');
  const processes: TrackingProcess[] = [];
  let complete = true, index = 0, bytes = 0;
  const deadline = Date.now() + SNAPSHOT_TIMEOUT;
  await Promise.all(Array.from({ length: Math.min(16, pids.length) }, async () => {
    while (index < pids.length) {
      signal?.throwIfAborted();
      if (Date.now() > deadline) throw new Error('Process snapshot timed out.');
      const pid = Number(pids[index++]!), directory = '/proc/' + pid;
      let before: ReturnType<typeof procStat>;
      try { before = procStat(await boundedRead(directory + '/stat', signal)); }
      catch (error) {
        if ((error as NodeJS.ErrnoException).code !== 'ENOENT' && (error as NodeJS.ErrnoException).code !== 'ESRCH') complete = false;
        continue;
      }
      if (before.dead) continue;
      let executable: string | null = null, argv: string[] | null = null, accessible = false;
      let marker: string | null | undefined;
      try {
        const status = await boundedRead(directory + '/status', signal);
        const users = /^Uid:\s+(\d+)\s+(\d+)/m.exec(status);
        if (users && Number(users[1]) === uid && Number(users[2]) === uid) {
          executable = (await readlink(directory + '/exe')).replace(/ \(deleted\)$/, '');
          accessible = true;
          try { argv = (await boundedRead(directory + '/cmdline', signal)).split('\0'); if (argv.at(-1) === '') argv.pop(); } catch { /* Relevant script rules will report unknown. */ }
          if (rootPids.has(pid)) {
            marker = null;
            try { marker = environmentMarker(await boundedRead(directory + '/environ', signal)); }
            catch { /* Missing root marker evidence remains unknown. */ }
          }
        }
      } catch { /* Keep ancestry and birth evidence for inaccessible descendants. */ }
      let birth: string | null = boot + ':' + before.ticks;
      try {
        const after = procStat(await boundedRead(directory + '/stat', signal));
        if (after.dead) continue;
        if (before.ticks !== after.ticks || before.parentPid !== after.parentPid) { birth = null; accessible = false; }
      } catch { birth = null; accessible = false; }
      const p = { pid, parentPid: before.parentPid, birth, birthOrder: before.ticks, executable, argv, accessible, marker, processName: before.name };
      bytes += Buffer.byteLength(JSON.stringify(p), 'utf8');
      if (bytes > MAX_OUTPUT) throw new Error('Process output limit exceeded.');
      processes.push(p);
    }
  }));
  return { processes, complete, rootMarkerRequired: true };
}

// Retain only the marker, never the full environ.
// No command, marker or distro is interpolated.
const WSL_PYTHON = String.raw`
import os, json, errno
LIMIT = 65536
def read(path):
    fd = os.open(path, os.O_RDONLY)
    with os.fdopen(fd, 'rb') as f:
        b = f.read(LIMIT + 1)
    if len(b) > LIMIT: raise ValueError('field limit')
    return b
def stat(path):
    b = read(path); f = b[b.rfind(b')')+2:].split()
    return int(f[1]), f[19].decode('ascii'), f[0] in (b'Z', b'X', b'x'), text(b[b.find(b'(')+1:b.rfind(b')')])
def text(b): return b.decode('utf-8', 'replace')
boot = text(read('/proc/sys/kernel/random/boot_id')).strip()
uid = os.getuid()
if uid != os.geteuid() or len(boot) != 36: raise ValueError('identity unavailable')
pids = [p for p in os.listdir('/proc') if p.isdigit()]
if len(pids) > 20000: raise ValueError('process limit')
rows = []; complete = True; total = 0
for number in pids:
    d = '/proc/' + number
    try: parent, ticks, dead, name = stat(d + '/stat')
    except OSError as e:
        if e.errno not in (errno.ENOENT, errno.ESRCH): complete = False
        continue
    if dead: continue
    exe = None; args = None; marker = None; accessible = False
    try:
        users = next(l.split()[1:3] for l in read(d + '/status').splitlines() if l.startswith(b'Uid:'))
        if all(int(u) == uid for u in users):
            exe = os.readlink(d + '/exe')
            if exe.endswith(' (deleted)'): exe = exe[:-10]
            accessible = True
            try:
                raw = read(d + '/cmdline').split(b'\0')
                if raw and raw[-1] == b'': raw.pop()
                args = [text(a) for a in raw]
            except (OSError, ValueError): pass
            try:
                env = read(d + '/environ').split(b'\0')
                markers = [text(e[len(b'SHELLFOX_TERMINAL_MARKER='):]) for e in env if e.startswith(b'SHELLFOX_TERMINAL_MARKER=')]
                if len(markers) == 1: marker = markers[0]
            except (OSError, ValueError): pass
    except (OSError, ValueError, StopIteration): pass
    birth = boot + ':' + ticks
    try:
        after = stat(d + '/stat')
        if after[2]: continue
        if after[:2] != (parent, ticks): birth = None; accessible = False
    except (OSError, ValueError): birth = None; accessible = False
    row = dict(pid=int(number), parentPid=parent, birth=birth, birthOrder=ticks, executable=exe, argv=args, accessible=accessible, marker=marker, processName=name)
    total += len(json.dumps(row))
    if total > 8388608: raise ValueError('output limit')
    rows.append(row)
print(json.dumps(dict(processes=rows, complete=complete)))
`;

// GNU grep -z reads NUL-separated environment records, not lines inside an env value.
// Base64 preserves whitespace, quotes, empty argv slots and control characters.
const WSL_SHELL = String.raw`
export LC_ALL=C
for tool in cat id readlink base64 grep head; do command -v "$tool" >/dev/null 2>&1 || exit 69; done
printf 'x\000' | grep -az '^x$' >/dev/null || exit 69
uid=$(id -u) || exit 69
boot=$(cat /proc/sys/kernel/random/boot_id) || exit 69
[ "${'$'}{#boot}" = 36 ] || exit 69
printf 'SHELLFOX_TRACKING_1\t%s\n' "$boot"
stat_fields() {
  rest=${'$'}{1##*) }
  set -- $rest
  [ "$#" -ge 20 ] || return 1
  state=$1; parent=$2; shift 19; ticks=$1
  case "$parent:$ticks" in *[!0-9:]*|:*) return 1;; esac
}
field() {
  value=$({ head -c 65537 "$1" 2>/dev/null && printf '\0001' || printf '\0000'; } | base64 -w0) || return 1
  [ "${'$'}{#value}" -le 87388 ] || return 1
  printf '%s' "$value"
}
n=0
for d in /proc/[0-9]*; do
  n=$((n+1)); [ "$n" -le 20000 ] || exit 70
  before=$(cat "$d/stat" 2>/dev/null) || { [ ! -d "$d" ] || printf 'E\n'; continue; }
  stat_fields "$before" || exit 70
  case "$state" in Z|X|x) continue;; esac
  pp=$parent; start=$ticks; ok=0; exe=''; args='-'; marker='-'
  name=${'$'}{before#*(}; name=${'$'}{name%)*}; name=$(printf '%s' "$name" | base64 -w0) || exit 70
  real=''; effective=''
  while read -r label a b rest; do
    if [ "$label" = 'Uid:' ]; then real=$a; effective=$b; break; fi
  done < "$d/status" 2>/dev/null
  if [ "$real" = "$uid" ] && [ "$effective" = "$uid" ]; then
    path=$(readlink "$d/exe" 2>/dev/null) && { exe=$(printf '%s' "$path" | base64 -w0); ok=1; }
    [ -r "$d/cmdline" ] && args=$(field "$d/cmdline" 2>/dev/null) || args='-'
    [ -r "$d/environ" ] && marker=$(head -c 65537 "$d/environ" | grep -az '^SHELLFOX_TERMINAL_MARKER=' | base64 -w0) || marker='-'
  fi
  after=$(cat "$d/stat" 2>/dev/null) || { printf 'E\n'; continue; }
  stat_fields "$after" || exit 70
  case "$state" in Z|X|x) continue;; esac
  if [ "$start" != "$ticks" ] || [ "$pp" != "$parent" ]; then printf 'E\n'; continue; fi
  printf 'R\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "${'$'}{d##*/}" "$pp" "$start" "$ok" "$exe" "$args" "$marker" "$name"
done
`;
function shellSnapshot(output: string): { processes: TrackingProcess[]; complete: boolean } {
  const lines = output.replace(/\n$/, '').split('\n');
  const header = lines.shift()!.split('\t');
  if (header[0] !== 'SHELLFOX_TRACKING_1' || !/^[a-f\d-]{36}$/i.test(header[1] ?? '')) throw new Error('Invalid guest boot identity.');
  const processes: TrackingProcess[] = [];
  let complete = true;
  const decode = (s: string) => {
    if (!/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(s) || s.length > 87388) throw new Error('Invalid guest field.');
    const bytes = Buffer.from(s, 'base64');
    if (bytes.length > MAX_FIELD + 2) throw new Error('Guest field limit exceeded.');
    return bytes.toString('utf8');
  };
  for (const line of lines) {
    if (line === 'E') { complete = false; continue; }
    const f = line.split('\t');
    if (![8, 9].includes(f.length) || f[0] !== 'R' || !/^\d+$/.test(f[3]!)) throw new Error('Invalid guest process.');
    const cmdline = f[6] === '-' ? null : decode(f[6]!);
    const args = cmdline?.endsWith('\0' + '1') ? cmdline.slice(0, -2).split('\0') : null;
    if (args?.at(-1) === '') args.pop();
    const env = f[7] === '-' ? [] : decode(f[7]!).split('\0').filter(Boolean);
    const prefix = 'SHELLFOX_TERMINAL_MARKER=';
    processes.push({
      pid: Number(f[1]), parentPid: Number(f[2]), birth: header[1] + ':' + f[3], birthOrder: f[3],
      accessible: f[4] === '1', executable: f[5] ? decode(f[5]!).replace(/ \(deleted\)$/, '') : null,
      processName: f.length === 9 ? decode(f[8]!) : null,
      argv: args, marker: env.length === 1 && env[0]!.startsWith(prefix) ? env[0]!.slice(prefix.length) : null,
    });
    if (processes.length > MAX_ROWS) throw new Error('Guest process limit exceeded.');
  }
  return { processes, complete };
}
async function wslSnapshot(distro: string, signal?: AbortSignal): Promise<{ processes: TrackingProcess[]; complete: boolean }> {
  if (process.platform !== 'win32' || !distro || distro.includes('\0')) throw new Error('WSL unavailable.');
  const exe = await windowsExecutable('wsl');
  try {
    const output = await command(exe, ['-d', distro, '--exec', 'python3', '-c', WSL_PYTHON], signal);
    const data = JSON.parse(output) as { processes: TrackingProcess[]; complete: boolean };
    if (!Array.isArray(data.processes) || typeof data.complete !== 'boolean') throw new Error('Invalid guest snapshot.');
    return data;
  } catch (error) {
    const e = error as NodeJS.ErrnoException & { killed?: boolean };
    if (signal?.aborted || e.killed || e.code === 'ERR_CHILD_PROCESS_STDIO_MAXBUFFER' || e.code === 'ENOENT') throw error;
    // Python is optional; the fixed proc fallback has its own dependency checks and bounds.
    return shellSnapshot(await command(exe, ['-d', distro, '--exec', 'sh', '-c', WSL_SHELL], signal));
  }
}
export interface TrackingHelperOptions {
  platform?: NodeJS.Platform;
  arch?: string;
  /** Electron resources path. Packaged mode never falls back to a development binary. */
  resourcesPath?: string;
  packaged?: boolean;
  projectRoot?: string;
}
export interface ProcessTrackingCapability { available: boolean; reason: string | null; helperPath: string | null }
const MAC_HELPER = 'shellfox-process-snapshot';
const MAC_SOURCE = 'libproc+numeric-sysctl';
const MAC_PREFLIGHT_MARKER = 'shellfox-helper-preflight-v1';

export function macTrackingHelperPath(options: TrackingHelperOptions = {}): string {
  const runtime = process as NodeJS.Process & { resourcesPath?: string; defaultApp?: boolean };
  const arch = options.arch ?? process.arch;
  const resources = options.resourcesPath ?? runtime.resourcesPath;
  const packaged = options.packaged ?? (!!resources && runtime.defaultApp !== true);
  if (packaged) {
    if (!resources) throw new Error('Packaged macOS tracking requires process.resourcesPath.');
    return join(resources, 'terminal-native', MAC_HELPER);
  }
  return resolve(options.projectRoot ?? process.cwd(), 'tmp', 'terminal-native', `darwin-${arch}`, MAC_HELPER);
}
function nativeHelperCommand(file: string, mode: '--capabilities' | '--snapshot', signal?: AbortSignal): Promise<string> {
  return new Promise((resolveOutput, reject) => {
    execFile(file, [mode], {
      encoding: 'utf8', timeout: mode === '--capabilities' ? 3000 : SNAPSHOT_TIMEOUT,
      killSignal: 'SIGKILL', maxBuffer: mode === '--capabilities' ? 16 * 1024 : MAX_OUTPUT,
      signal, env: mode === '--capabilities' ? { ...process.env, SHELLFOX_TERMINAL_MARKER: MAC_PREFLIGHT_MARKER } : process.env,
    }, (error, stdout) => error ? reject(error) : resolveOutput(stdout));
  });
}
function helperFailure(file: string, error: unknown): string {
  const e = error as NodeJS.ErrnoException & { killed?: boolean };
  if (e.code === 'ENOENT') return `macOS process tracking helper is missing: ${file}`;
  if (e.code === 'EACCES') return `macOS process tracking helper is not executable: ${file}`;
  if (e.code === 'ENOEXEC') return `macOS process tracking helper has an incompatible executable format: ${file}`;
  if (e.killed || e.code === 'ABORT_ERR' || e.code === 'ERR_CHILD_PROCESS_STDIO_MAXBUFFER') return 'macOS process tracking helper was aborted or exceeded its timeout/output bound.';
  return `macOS process tracking helper execution failed at ${file}.`;
}
function nativeEnvelope(raw: string, kind: 'snapshot' | 'capabilities', arch: string): Record<string, unknown> {
  if (Buffer.byteLength(raw) > MAX_OUTPUT) throw new Error('Native helper output exceeded its bound.');
  const value: unknown = JSON.parse(raw);
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Invalid native helper envelope.');
  const d = value as Record<string, unknown>;
  if (d.protocol !== 1 || d.platform !== 'darwin' || d.arch !== arch || d.source !== MAC_SOURCE || d.kind !== kind ||
    !Number.isInteger(d.ownerUid) || Number(d.ownerUid) < 0 || Number(d.ownerUid) > 0xffffffff ||
    process.getuid && d.ownerUid !== process.getuid() ||
    d.reason !== null && (typeof d.reason !== 'string' || !d.reason || d.reason.length > 1000 || /[\u0000-\u001f\u007f]/.test(d.reason))) {
    throw new Error('Incompatible native helper protocol, architecture, or credentials.');
  }
  return d;
}
/** Service owner must use this result for probe.capabilities.processTracking.
 * A readable helper file alone is not readiness; run actual native self preflight.
 * No package/dependency install, shell, ps fallback, or dotted CLI sysctl OID. */
export async function getProcessTrackingCapability(options: TrackingHelperOptions = {}, signal?: AbortSignal): Promise<ProcessTrackingCapability> {
  const platform = options.platform ?? process.platform;
  if (platform === 'win32') {
    try { await windowsExecutable('powershell'); return { available: true, reason: null, helperPath: null }; }
    catch { return { available: false, reason: 'Windows PowerShell/CIM process inspection executable is unavailable.', helperPath: null }; }
  }
  if (platform === 'linux') {
    try {
      if (process.getuid?.() === undefined || process.getuid() !== process.geteuid?.()) throw new Error('Same-user evidence unavailable.');
      const boot = (await boundedRead('/proc/sys/kernel/random/boot_id', signal)).trim();
      if (!/^[a-f\d-]{36}$/i.test(boot)) throw new Error('Invalid boot identity.');
      await access('/proc/self/stat', constants.R_OK);
      return { available: true, reason: null, helperPath: null };
    } catch { return { available: false, reason: 'Same-user Linux /proc birth inspection is unavailable.', helperPath: null }; }
  }
  if (platform !== 'darwin') return { available: false, reason: 'Process tracking is unsupported on this platform.', helperPath: null };
  const arch = options.arch ?? process.arch;
  if (!['x64', 'arm64'].includes(arch)) return { available: false, reason: `macOS native process tracking does not support architecture ${arch}.`, helperPath: null };
  let file: string;
  try { file = macTrackingHelperPath(options); }
  catch { return { available: false, reason: 'Packaged macOS process tracking helper resources path is unavailable.', helperPath: null }; }
  try { await access(file, constants.X_OK); }
  catch (error) { return { available: false, reason: helperFailure(file, error), helperPath: file }; }
  let output: string;
  try { output = await nativeHelperCommand(file, '--capabilities', signal); }
  catch (error) { return { available: false, reason: helperFailure(file, error), helperPath: file }; }
  try {
    const d = nativeEnvelope(output, 'capabilities', arch);
    if (typeof d.available !== 'boolean' || d.exactBirth !== true || d.identityAccess !== true || d.enumeration !== true ||
      d.argv !== true || d.environmentMarker !== true || d.termination !== false || d.available && d.reason !== null) {
      throw new Error('Invalid native capability response.');
    }
    return { available: d.available, reason: d.available ? null : d.reason as string ?? 'macOS native helper self preflight failed.', helperPath: file };
  } catch { return { available: false, reason: 'macOS process tracking helper returned an incompatible capability protocol or architecture.', helperPath: file }; }
}

function parseMacSnapshot(output: string, arch: string): { processes: TrackingProcess[]; complete: boolean; reason?: string; rootMarkerRequired: true } {
  const d = nativeEnvelope(output, 'snapshot', arch);
  if (!Array.isArray(d.processes) || d.processes.length > MAX_ROWS || typeof d.complete !== 'boolean') throw new Error('Invalid native process collection.');
  const seen = new Set<number>();
  const processes = d.processes.map((raw: unknown): TrackingProcess => {
    if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw new Error('Invalid native process.');
    const p = raw as Record<string, unknown>;
    const integer = (v: unknown, max: number, nullable = false): boolean => nullable && v === null || Number.isInteger(v) && Number(v) >= 0 && Number(v) <= max;
    if (!integer(p.pid, 0x7fffffff) || Number(p.pid) <= 0 || !integer(p.parentPid, 0x7fffffff) || seen.has(Number(p.pid)) ||
      !integer(p.pgid, 0x7fffffff, true) || !integer(p.sid, 0x7fffffff, true) ||
      !integer(p.uid, 0xffffffff, true) || !integer(p.realUid, 0xffffffff, true) || !integer(p.savedUid, 0xffffffff, true) ||
      typeof p.identityVerified !== 'boolean' || typeof p.accessible !== 'boolean' ||
      p.executable !== null && (typeof p.executable !== 'string' || !p.executable.startsWith('/') || Buffer.byteLength(p.executable) > MAX_FIELD || p.executable.includes('\0')) ||
      p.argv !== null && (!Array.isArray(p.argv) || p.argv.length > 4096 || p.argv.some(a => typeof a !== 'string' || a.includes('\0')) ||
        Buffer.byteLength(p.argv.join('\0')) > MAX_FIELD) ||
      p.processName != null && (typeof p.processName !== 'string' || p.processName.includes('\0') || Buffer.byteLength(p.processName) > MAX_FIELD) ||
      p.marker !== null && (typeof p.marker !== 'string' || p.marker.length > 1024 || p.marker.includes('\0'))) throw new Error('Invalid native process fields.');
    seen.add(Number(p.pid));
    let birth: string | null = null, birthOrder: string | undefined;
    if (p.identityVerified) {
      if (typeof p.startSeconds !== 'string' || !/^[1-9]\d{0,19}$/.test(p.startSeconds) || BigInt(p.startSeconds) > 0xffffffffffffffffn ||
        typeof p.startMicroseconds !== 'string' || !/^\d{1,6}$/.test(p.startMicroseconds) || BigInt(p.startMicroseconds) >= 1_000_000n ||
        p.uid === null || p.realUid === null || p.savedUid === null || p.sid === null || Number(p.sid) <= 0 || p.pgid === null) throw new Error('Invalid exact native birth evidence.');
      birth = `darwin:${p.startSeconds}:${p.startMicroseconds.padStart(6, '0')}`;
      birthOrder = (BigInt(p.startSeconds) * 1_000_000n + BigInt(p.startMicroseconds)).toString();
    } else if (p.startSeconds !== null || p.startMicroseconds !== null || p.accessible) throw new Error('Unverified native birth claimed as accessible.');
    if (p.accessible && (!p.identityVerified || !p.executable || p.uid !== d.ownerUid || p.realUid !== d.ownerUid || p.savedUid !== d.ownerUid)) {
      throw new Error('Native process access/credential evidence is inconsistent.');
    }
    return { pid: Number(p.pid), parentPid: Number(p.parentPid), birth, birthOrder, executable: p.executable as string | null,
      argv: p.argv as string[] | null, accessible: p.accessible, marker: p.marker as string | null,
      processName: p.processName as string | null | undefined,
      pgid: p.pgid as number | null, sid: p.sid as number | null, uid: p.uid as number | null, realUid: p.realUid as number | null, savedUid: p.savedUid as number | null };
  });
  return { processes, complete: d.complete, reason: d.reason as string ?? undefined, rootMarkerRequired: true };
}
async function macSnapshot(options: TrackingHelperOptions, capability: ProcessTrackingCapability, signal?: AbortSignal): Promise<{
  processes: TrackingProcess[]; complete: boolean; reason?: string; rootMarkerRequired: true;
}> {
  const unavailable = (reason: string) => ({ processes: [], complete: false, rootMarkerRequired: true as const, reason });
  if (!capability.available || !capability.helperPath) return unavailable(capability.reason ?? 'macOS process tracking helper is unavailable.');
  let output: string;
  try { output = await nativeHelperCommand(capability.helperPath, '--snapshot', signal); }
  catch (error) { return unavailable(helperFailure(capability.helperPath, error)); }
  try { return parseMacSnapshot(output, options.arch ?? process.arch); }
  catch { return unavailable('macOS process tracking helper returned malformed or incompatible snapshot evidence.'); }
}

export function createSystemSnapshotProvider(helperOptions: TrackingHelperOptions = {}): SnapshotProvider {
  const windowsKnown = new Map<number, string>();
  let macCapability: ProcessTrackingCapability | undefined;
  return async (roots, signal) => {
    const targets = [...new Map(roots.map(r => [JSON.stringify([r.environment, r.distro]), r])).values()];
    const snapshots: TrackingSnapshot[] = targets.map(r => ({
      domain: JSON.stringify([hostname(), r.environment, r.distro]), environment: r.environment,
      distro: r.distro, processes: [], complete: false,
    }));
    if (targets.length > 32) return snapshots.map(s => ({ ...s, reason: 'Process namespace limit exceeded.' }));
    let index = 0;
    await Promise.all(Array.from({ length: Math.min(4, targets.length) }, async () => {
      while (index < targets.length) {
        const i = index++, r = targets[i]!, snapshot = snapshots[i]!;
        try {
          signal?.throwIfAborted();
          if (r.environment === 'wsl') Object.assign(snapshot, await wslSnapshot(r.distro!, signal));
          else if (process.platform === 'win32') {
            snapshot.processes = await windowsSnapshot(roots.filter(r => r.environment === 'local'), windowsKnown, signal);
            snapshot.complete = true;
            snapshot.rootLaunchEvidenceRequired = true;
          } else if (process.platform === 'linux') Object.assign(snapshot, await linuxSnapshot(roots.filter(r => r.environment === 'local'), signal));
          else if (process.platform === 'darwin') {
            // Success can be cached for immutable packaged resources. Failures are
            // rechecked so development builds can install the helper without a restart.
            if (!macCapability?.available) macCapability = await getProcessTrackingCapability({ ...helperOptions, platform: 'darwin' }, signal);
            const result = await macSnapshot(helperOptions, macCapability, signal);
            if (!result.complete && !result.processes.length) macCapability = undefined;
            Object.assign(snapshot, result);
          }
          else snapshot.reason = 'Process inspection is unsupported on this platform.';
        } catch {
          snapshot.reason = r.environment === 'wsl' ? 'Guest process inspection unavailable; WSL or guest snapshot dependencies failed.' : 'Host process inspection unavailable or exceeded its bounds.';
          snapshot.processes = [];
          snapshot.complete = false;
        }
      }
    }));
    if (process.platform === 'win32' && roots.some(r => r.marker) && !signal?.aborted) {
      // Direct WSL snapshots are reused. Optional collection has a total budget,
      // four workers and at most 32 running distros. Listing never boots a distro.
      const optionalSignal = AbortSignal.any([...(signal ? [signal] : []), AbortSignal.timeout(3000)]);
      try {
        const output = await command(await windowsExecutable('wsl'), ['--list', '--running', '--quiet'], optionalSignal, undefined, true);
        const running = [...new Set(output.replace(/^\uFEFF/, '').split(/\r?\n/).map(s => s.trim()).filter(Boolean))];
        if (running.length > 32 || running.some(d => d.length > 256 || /[\x00-\x1f\x7f]/.test(d))) throw new Error('Invalid running distro list.');
        const pending = running.filter(d => !snapshots.some(s => s.environment === 'wsl' && s.distro === d));
        let next = 0;
        await Promise.all(Array.from({ length: Math.min(4, pending.length) }, async () => {
          while (next < pending.length && !optionalSignal.aborted) {
            const distro = pending[next++]!;
            try {
              const result = await wslSnapshot(distro, optionalSignal);
              snapshots.push({ domain: JSON.stringify([hostname(), 'wsl', distro]), environment: 'wsl', distro, statusOnly: true, ...result });
            } catch { /* Optional status evidence cannot make unrelated tabs unhealthy. */ }
          }
        }));
      } catch { /* WSL unavailable, enumeration failed or optional budget expired. */ }
    }
    return snapshots;
  };
}

export const systemSnapshotProvider: SnapshotProvider = createSystemSnapshotProvider();
