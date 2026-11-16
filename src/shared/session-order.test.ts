import { describe, expect, it } from 'vitest';
import type { SessionDto } from './contracts';
import { sessionDot } from './terminal-status';
import { sortVisibleSessions } from './session-order';
import { session } from '../renderer/test-fixtures';

const ids = (items: SessionDto[]) => items.map(item => item.id);
describe('visible session sorting', () => {
  it('puts pins first, then error, solid green, flashing green, blue and settled, newest first', () => {
    const shell = session(1);
    const running = session(2); running.tabs[0]!.status = 'running';
    const busy = session(3); busy.tabs[0]!.status = 'running';
    const settled = session(4, { status: 'settled', settledAt: '2026-10-04T11:00:00.000Z' });
    const error = session(5, { error: { code: 'INTERNAL', message: 'Oops', retryable: true } });
    const pinned = session(6, { pinnedAt: '2026-10-04T11:00:00.000Z' });
    const pinnedSettled = session(7, { ...settled, id: session(7).id, title: 'Session 7', pinnedAt: '2026-10-04T10:00:00.000Z' });
    const input = [shell, busy, settled, running, pinned, error, pinnedSettled];
    expect(ids(sortVisibleSessions(input, { [busy.tabs[0]!.id]: true }))).toEqual(ids([pinned, pinnedSettled, error, running, busy, shell, settled]));
    expect(input[0]).toBe(shell);
  });

  it('orders one status by newest creation, then name, with waiting and unknown ranked together', () => {
    const unknown = session(1, { status: 'unknown', activityStatus: 'unknown', title: 'beta' }); unknown.tabs[0]!.status = 'unknown';
    const named = session(2, { title: 'Alpha' });
    const older = session(3, { createdAt: '2026-01-01T00:00:00.000Z', title: 'aaa' });
    const newer = session(4, { createdAt: '2026-12-01T00:00:00.000Z', title: 'zzz' });
    expect(ids(sortVisibleSessions([older, unknown, named, newer], {}))).toEqual(ids([newer, named, unknown, older]));
  });

  it('keeps a row in place while only its activity timestamp changes', () => {
    const first = session(1), second = session(2, { createdAt: '2026-10-05T10:00:00.000Z' });
    const before = ids(sortVisibleSessions([first, second], {}));
    first.updatedAt = '2026-10-09T10:00:00.000Z';
    expect(ids(sortVisibleSessions([second, first], {}))).toEqual(before);
    expect(before).toEqual(ids([second, first]));
  });

  it('reorders only when the displayed class changes on activity updates', () => {
    const first = session(1), second = session(2);
    first.tabs[0]!.status = second.tabs[0]!.status = 'running';
    expect(ids(sortVisibleSessions([first, second], { [first.tabs[0]!.id]: true }))).toEqual(ids([second, first]));
    expect(ids(sortVisibleSessions([first, second], {}))).toEqual(ids([first, second]));
  });

  it('always shows settled brown even with live agents, errors, or busy terminals', () => {
    const item = session(1, { status: 'settled', settledAt: '2026-10-04T11:00:00.000Z', error: { code: 'INTERNAL', message: 'Oops', retryable: true } });
    item.tabs[0]!.status = 'running';
    expect(sessionDot(item, { [item.tabs[0]!.id]: true })).toEqual({ kind: 'settled', title: 'Settled' });
  });
});
