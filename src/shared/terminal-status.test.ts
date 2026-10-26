import { describe, expect, it } from 'vitest';
import type { AppError, SessionDto, TabDto } from './contracts';
import { sessionDot, tabDot } from './terminal-status';

const error: AppError = { code: 'LAUNCH_FAILED', message: 'Failed', retryable: true };
const tab = (overrides: Partial<TabDto> = {}): TabDto => ({
  id: 'tab', sessionId: 'session', title: 'Shell', cwd: '/work', ordinal: 0,
  createdAt: '2026-01-01', lifecycle: 'open', status: 'waiting', agents: 0, monitoringReason: null, error: null,
  ...overrides,
});
const session = (overrides: Partial<SessionDto> = {}): SessionDto => ({
  id: 'session', title: 'work', cwd: '/work', adapterId: 'embedded-pty', shellId: 'bash',
  createdAt: '2026-01-01', updatedAt: '2026-01-01', settledAt: null, pinnedAt: null,
  status: 'waiting', activityStatus: 'waiting', error: null, env: [], tabs: [tab()],
  counts: { running: 0, waiting: 1, unknown: 0, error: 0, closed: 0, agents: 0 },
  canFocus: false, canAddTab: true, controlReason: null, ...overrides,
});

describe('shared desktop and mobile terminal dots', () => {
  it.each(['waiting', 'unknown'] as const)('uses shell blue for %s even with activity', status => {
    expect(tabDot(tab({ status, agents: 5 }), true)).toEqual({ kind: 'shell', title: 'Shell' });
  });
  it('distinguishes idle and working agents only through terminal activity', () => {
    expect(tabDot(tab({ status: 'running' }))).toEqual({ kind: 'running', title: 'Agent idle' });
    expect(tabDot(tab({ status: 'running' }), true)).toEqual({ kind: 'busy', title: 'Agent working' });
  });
  it('keeps tab errors ahead of running agents', () => {
    expect(tabDot(tab({ status: 'running', error }), true)).toEqual({ kind: 'error', title: 'Error: Failed' });
    expect(tabDot(tab({ status: 'error' }))).toEqual({ kind: 'error', title: 'Error' });
  });
  it('settled wins over sticky and tab errors', () => {
    expect(sessionDot(session({ status: 'settled', error }), {})).toEqual({ kind: 'settled', title: 'Settled' });
    expect(sessionDot(session({ settledAt: '2026-01-02', error }), {})).toEqual({ kind: 'settled', title: 'Settled' });
  });
  it('uses sticky session errors even without any live tabs', () => {
    expect(sessionDot(session({ tabs: [], error }), {})).toEqual({ kind: 'error', title: 'Error: Failed' });
  });
  it('ignores closed tabs and keeps empty sessions blue', () => {
    expect(sessionDot(session({ tabs: [] }), {})).toEqual({ kind: 'shell', title: 'Shell' });
    expect(sessionDot(session({ tabs: [tab({ lifecycle: 'closed', status: 'error', error })] }), {})).toEqual({ kind: 'shell', title: 'Shell' });
  });
  it('prioritizes errors, working agents, idle agents, then shells across live tabs', () => {
    const tabs = [tab(), tab({ id: 'idle', status: 'running' }), tab({ id: 'busy', status: 'running' })];
    expect(sessionDot(session({ tabs }), {}).kind).toBe('running');
    expect(sessionDot(session({ tabs }), { busy: true }).kind).toBe('busy');
    expect(sessionDot(session({ tabs: [...tabs, tab({ id: 'error', status: 'error' })] }), { busy: true }).kind).toBe('error');
  });
});
