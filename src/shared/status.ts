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
export const publicStatus = (activity: TabStatus, settledAt: string | null): Status => settledAt ? 'settled' : activity;
// Empty sessions and ordinary shells share the same blue status and sort rank.
const order: Record<Status, number> = { running: 0, error: 1, waiting: 2, unknown: 2, settled: 3 };
type Sortable = Pick<SessionDto, 'status' | 'createdAt' | 'id'> & { pinnedAt?: string | null };
/** Pin order first, then visible status and creation order. Terminal count never breaks ties. */
export function compareSessions(a: Sortable, b: Sortable): number {
  const pinned = (a.pinnedAt ? 0 : 1) - (b.pinnedAt ? 0 : 1);
  if (pinned) return pinned;
  if (a.pinnedAt && b.pinnedAt && a.pinnedAt !== b.pinnedAt) return a.pinnedAt.localeCompare(b.pinnedAt);
  return order[a.status] - order[b.status] || a.createdAt.localeCompare(b.createdAt) || a.id.localeCompare(b.id);
}
export const sortSessions = (sessions: SessionDto[]): SessionDto[] => [...sessions].sort(compareSessions);
