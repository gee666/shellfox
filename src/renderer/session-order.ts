import type { SessionDto } from '../shared/contracts';
import { sessionDot } from './components';
import type { DotState } from './components';

const order: Record<DotState['kind'], number> = { error: 0, running: 1, busy: 2, shell: 3, settled: 4 };

/** Sort by the status users see, not the underlying waiting/unknown distinction. */
export function sortVisibleSessions(sessions: SessionDto[], busy: Record<string, boolean>): SessionDto[] {
  return sessions.map(session => ({ session, kind: sessionDot(session, busy).kind }))
    .sort((a, b) => {
      const aPin = a.session.pinnedAt;
      const bPin = b.session.pinnedAt;
      const pinned = (aPin ? 0 : 1) - (bPin ? 0 : 1);
      if (pinned) return pinned;
      if (aPin && bPin && aPin !== bPin) return aPin.localeCompare(bPin);
      return order[a.kind] - order[b.kind] || a.session.createdAt.localeCompare(b.session.createdAt) || a.session.id.localeCompare(b.session.id);
    }).map(item => item.session);
}
