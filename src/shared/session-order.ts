import type { SessionDto } from './contracts';
import { sessionDot, type DotState } from './terminal-status';
import { compareCreation } from './status';

const order: Record<DotState['kind'], number> = { error: 0, running: 1, busy: 2, shell: 3, settled: 4 };

/** Pinned first, then visible status, newest creation, and name. Activity timestamps never reorder rows. */
export function sortVisibleSessions(sessions: readonly SessionDto[], busy: Readonly<Record<string, boolean>>): SessionDto[] {
  return sessions.map(session => ({ session, kind: sessionDot(session, busy).kind }))
    .sort((a, b) => (a.session.pinnedAt ? 0 : 1) - (b.session.pinnedAt ? 0 : 1) || order[a.kind] - order[b.kind]
      || compareCreation(a.session, b.session))
    .map(item => item.session);
}
