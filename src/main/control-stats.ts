import type { TabStat, ProcessStat } from './control-types';
import type { PtyBackend } from './terminal/backend';
import type { TabObservation } from '../shared/native-port';
import { ProcessTracker, createSystemSnapshotProvider, type SnapshotProvider, type TrackingSnapshot } from './terminal/tracking';
import { collectSystemMetrics, type MetricsProvider } from './control-stats-metrics';

type LiveTerminal = ReturnType<PtyBackend['live']>[number];
const identityKey = (p: { domain: string; pid: number; birth: string }) => JSON.stringify([p.domain, p.pid, p.birth]);

/** Keeps birth/parent continuity across requests without starting tracker polling.
 * Calls are serialized so a late snapshot cannot change another request's scope.
 * Ownership history starts at the first request. Already-detached descendants
 * cannot be recovered from current ancestry alone. */
export function createSessionStatsCollector(
  snapshotProvider: SnapshotProvider = createSystemSnapshotProvider(),
  metricsProvider: MetricsProvider = collectSystemMetrics,
): (live: readonly LiveTerminal[]) => Promise<TabStat[]> {
  let snapshots: TrackingSnapshot[] = [];
  let observations: TabObservation[] = [];
  let signal: AbortSignal;
  const tracker = new ProcessTracker(items => { observations = items; }, async roots => {
    snapshots = await snapshotProvider(roots, signal);
    return snapshots;
  });
  tracker.setVisibleSessions([]);
  let pending: Promise<unknown> = Promise.resolve();
  return live => {
    // backend.live() is already a copy, but callers can retain/mutate it.
    const entries = live.map(e => ({ ...e, root: { ...e.root, birthOrderBounds: e.root.birthOrderBounds && { ...e.root.birthOrderBounds } } }));
    const work = pending.then(async (): Promise<TabStat[]> => {
      snapshots = []; observations = [];
      signal = AbortSignal.timeout(20_000);
      const roots = entries.map(e => e.root);
      try { tracker.setWatch(roots, []); await tracker.poll(); }
      catch { return entries.map(e => ({ tabId: e.tabId, processes: [], memoryBytes: null, elapsedSeconds: null, reason: 'Process ownership snapshot unavailable or invalid.' })); }
      if (!entries.length) return [];
      const identities = roots.map(r => tracker.resolveIdentity(r));
      const claims = new Map<string, number>();
      for (const id of identities) if (id) claims.set(identityKey(id), (claims.get(identityKey(id)) ?? 0) + 1);
      const owned = roots.map((r, i) => {
        const id = identities[i];
        if (!id || claims.get(identityKey(id)) !== 1) return [];
        return [id, ...tracker.resolveDescendants(r)];
      });
      // Do not inspect unrelated host processes or optional status-only guests.
      const requested = new Set(owned.flat().map(identityKey));
      const scoped = snapshots.filter(s => !s.statusOnly && snapshots.filter(other => other.domain === s.domain).length === 1)
        .map(s => ({ ...s, processes: s.processes.filter(p => p.birth !== null && requested.has(identityKey({ domain: s.domain, pid: p.pid, birth: p.birth }))) }))
        .filter(s => s.processes.length);
      let metrics: Awaited<ReturnType<MetricsProvider>> = [];
      try { if (scoped.length) metrics = await metricsProvider(scoped, roots, signal); }
      catch { /* Keep verified process identities, never invent measurements. */ }
      return entries.map((e, i): TabStat => {
        const rootIdentity = identities[i];
        const reasons = new Set<string>();
        const observed = observations.find(o => o.tabId === e.tabId);
        if (observed?.reason) reasons.add(observed.reason);
        if (!rootIdentity || claims.get(identityKey(rootIdentity)) !== 1) reasons.add('Owned root identity unavailable or ambiguous.');
        const processes: ProcessStat[] = [];
        let elapsedSeconds: number | null = null;
        for (const id of owned[i]) {
          const matches = scoped.filter(s => s.domain === id.domain);
          if (matches.length !== 1) continue;
          const s = matches[0]!;
          if (!s.complete) reasons.add(s.reason ?? 'Process enumeration was incomplete.');
          const row = s.processes.find(p => p.pid === id.pid && p.birth === id.birth);
          if (!row) continue; // Retained owner knowledge is not proof of current life.
          const domains = metrics.filter(m => m.domain === id.domain);
          const domain = domains.length === 1 ? domains[0] : undefined;
          if (domain?.reason) reasons.add(domain.reason);
          const candidates = domain?.processes.filter(p => p.pid === id.pid && p.birth === id.birth && p.parentPid === row.parentPid) ?? [];
          const measured = candidates.length === 1 && row.accessible ? candidates[0] : undefined;
          const memory = measured?.memoryBytes;
          const runtime = measured?.elapsedSeconds;
          const memoryBytes = typeof memory === 'number' && Number.isSafeInteger(memory) && memory >= 0 ? memory : null;
          const duration = typeof runtime === 'number' && Number.isFinite(runtime) && runtime >= 0 ? runtime : null;
          if (memoryBytes === null) reasons.add('Resident memory unavailable for one or more owned processes.');
          if (duration === null) reasons.add('Runtime unavailable for one or more owned processes.');
          processes.push({ pid: row.pid, parentPid: row.parentPid,
            name: row.processName || row.executable?.replace(/\\/g, '/').split('/').pop() || 'n/a',
            memoryBytes, elapsedSeconds: duration, domain: id.domain });
          if (rootIdentity && identityKey(id) === identityKey(rootIdentity)) elapsedSeconds = duration;
        }
        const rootCurrent = rootIdentity && processes.some(p => p.domain === rootIdentity.domain && p.pid === rootIdentity.pid);
        // A closed PTY may remain live() while descendant cleanup is pending.
        // Complete, exact-birth exit evidence lets us sum its remaining owners;
        // an absent root is not an unavailable descendant memory measurement.
        const closedRootVerified = e.state === 'closed' && e.cleanupPending && !!rootIdentity &&
          observed?.root === 'exited' && observed.reason === null;
        if (!rootCurrent) reasons.add(closedRootVerified
          ? 'Owned root has exited; root runtime is unavailable.'
          : 'Owned root is no longer present or could not be verified.');
        processes.sort((a, b) => a.domain.localeCompare(b.domain) || a.pid - b.pid);
        // Sum RSS, not virtual size or a partial total. Shared pages may be
        // counted by each process; this is not unique physical memory usage.
        let memoryBytes: number | null = processes.length > 0 && processes.every(p => p.memoryBytes !== null)
          ? processes.reduce((total, p) => total + p.memoryBytes!, 0) : null;
        // Tracking health also requires the launched interactive executable.
        // After exec(), a previously pinned exact birth still owns this process
        // and its proven descendants. This specific status failure is not a
        // missing memory measurement. Other ownership/enumeration failures stay
        // unknown, including inaccessible descendants and reused roots.
        const rootExecChanged = rootCurrent && observed?.reason ===
          'Owned root is not the launched interactive shell; its executable changed or is unverified.';
        if (rootExecChanged) {
          reasons.delete(observed!.reason!);
          reasons.add('Owned root executable changed; its exact process birth is still verified.');
        }
        const enumerationUnknown = !observed || !rootExecChanged && !closedRootVerified && (observed.root !== 'alive' || observed.health !== 'healthy');
        if (enumerationUnknown || scoped.some(s => s.domain === rootIdentity?.domain && !s.complete)) memoryBytes = null;
        if (memoryBytes !== null && !Number.isSafeInteger(memoryBytes)) { memoryBytes = null; reasons.add('Resident memory total exceeds safe integer bounds.'); }
        return { tabId: e.tabId, processes, memoryBytes, elapsedSeconds, ...(reasons.size ? { reason: [...reasons].join(' ') } : {}) };
      });
    });
    pending = work.catch(() => {});
    return work;
  };
}

let collector: ReturnType<typeof createSessionStatsCollector> | undefined;
export function collectSessionStats(live: readonly LiveTerminal[]): Promise<TabStat[]> {
  collector ??= createSessionStatsCollector();
  return collector(live);
}
