import { describe, expect, it } from 'vitest';
import type { SessionDto } from '../shared/contracts';
import { sessionDot } from './components';
import { sortVisibleSessions } from './session-order';
import { session } from './test-fixtures';

const ids = (items: SessionDto[]) => items.map(item => item.id);
describe('visible session sorting', () => {
  it('puts every pin first, then error, solid green, flashing green, blue, and settled', () => {
    const shell = session(1);
    const running = session(2); running.tabs[0]!.status = 'running';
    const busy = session(3); busy.tabs[0]!.status = 'running';
    const settled = session(4, { status: 'settled', settledAt: '2026-10-04T11:00:00.000Z' });
    const error = session(5, { error: { code: 'INTERNAL', message: 'Oops', retryable: true } });
    const pinned = session(6, { pinnedAt: '2026-10-04T11:00:00.000Z' });
    const pinnedSettled = session(7, { ...settled, id: session(7).id, pinnedAt: '2026-10-04T10:00:00.000Z' });
    const input = [shell, busy, settled, running, pinned, error, pinnedSettled];
    expect(ids(sortVisibleSessions(input, { [busy.tabs[0]!.id]: true }))).toEqual(ids([pinnedSettled, pinned, error, running, busy, shell, settled]));
    expect(input[0]).toBe(shell);
  });

  it('uses the same rank for waiting and unknown, with stable creation/id ties', () => {
    const first = session(1, { status: 'unknown', activityStatus: 'unknown' }); first.tabs[0]!.status = 'unknown';
    const next = session(2);
    const older = session(3, { createdAt: '2026-01-01T00:00:00.000Z' });
    expect(ids(sortVisibleSessions([next, first, older], {}))).toEqual(ids([older, first, next]));
    first.status = 'waiting'; first.tabs[0]!.status = 'waiting'; next.status = 'unknown'; next.tabs[0]!.status = 'unknown';
    expect(ids(sortVisibleSessions([next, first, older], {}))).toEqual(ids([older, first, next]));
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
