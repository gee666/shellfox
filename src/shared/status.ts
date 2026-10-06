import type { AppError, Counts, SessionDto, Status, TabDto, TabStatus } from './contracts';
import type { TabObservation } from './native-port';
export function tabStatus(lifecycle: TabDto['lifecycle'], error: AppError | null, observation?: TabObservation): TabStatus {
  if (error) return 'error';
  if (lifecycle !== 'open' || !observation || observation.root !== 'alive') return 'unknown';
  if (observation.agents > 0) return 'running';
  return observation.health === 'healthy' ? 'waiting' : 'unknown';
}
export function aggregateStatus(tabs: Pick<TabDto, 'lifecycle' | 'status'>[], error: AppError | null = null): TabStatus {
  if (error || tabs.some(t => t.status === 'error')) return 'error';
  const live = tabs.filter(t => t.lifecycle !== 'closed');
  if (live.some(t => t.status === 'running')) return 'running';
  if (!live.length || live.some(t => t.status === 'unknown')) return 'unknown';
  return 'waiting';
}
export function countTabs(tabs: Pick<TabDto, 'lifecycle' | 'status' | 'agents'>[]): Counts {
  const counts: Counts = { running: 0, waiting: 0, unknown: 0, error: 0, closed: 0, agents: 0 };
  for (const tab of tabs) {
    if (tab.lifecycle === 'closed') counts.closed++;
    if (tab.lifecycle !== 'closed' || tab.status === 'error') counts[tab.status]++;
    if (tab.lifecycle !== 'closed') counts.agents += tab.agents;
  }
  return counts;
}
// status is the underlying activity, computed without a sticky launch/control error.
export function needsSettleConfirmation(tabs: Pick<TabDto, 'lifecycle' | 'status' | 'agents'>[]): boolean {
  return tabs.some(t => t.lifecycle !== 'closed' && (t.lifecycle !== 'open' || t.agents > 0 || t.status === 'running' || t.status === 'unknown'));
}
export const publicStatus = (activity: TabStatus, settledAt: string | null): Status => settledAt ? 'settled' : activity;
const order: Record<Status, number> = { waiting: 0, error: 1, running: 2, unknown: 3, settled: 4 };
type Sortable = Pick<SessionDto, 'status' | 'createdAt' | 'id'> & { pinnedAt?: string | null };
/** Pinned (live) sessions first, in the order they were pinned; then by status and age. */
export function compareSessions(a: Sortable, b: Sortable): number {
  const pinned = (a.pinnedAt ? 0 : 1) - (b.pinnedAt ? 0 : 1);
  if (pinned) return pinned;
  if (a.pinnedAt && b.pinnedAt && a.pinnedAt !== b.pinnedAt) return a.pinnedAt.localeCompare(b.pinnedAt);
  return order[a.status] - order[b.status] || a.createdAt.localeCompare(b.createdAt) || a.id.localeCompare(b.id);
}
export const sortSessions = (sessions: SessionDto[]): SessionDto[] => [...sessions].sort(compareSessions);
