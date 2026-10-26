import { describe, expect, it } from 'vitest';
import { connectionLabel, connectionSummary, isConnectionFailure } from './connection-status';

describe('live connection labels', () => {
  it.each([
    ['connected', 'connected'], ['disconnected', 'disconnected'], ['connecting', 'connecting...'],
    ['reconnecting', 'reconnecting...'], ['awaiting-approval', 'awaiting approval'], ['rejected', 'access denied'],
  ] as const)('maps %s without keeping an obsolete error', (state, expected) => {
    expect(connectionLabel(state)).toBe(expected);
  });
  it('distinguishes startup, no connection, and several hosts', () => {
    expect(connectionSummary([], true)).toBe('connecting...');
    expect(connectionSummary([])).toBe('disconnected');
    expect(connectionSummary(['connected', 'connected'])).toBe('connected');
    expect(connectionSummary(['connected', 'connecting', 'connected'])).toBe('2 connected · 1 connecting...');
    expect(connectionSummary(['reconnecting', 'disconnected'])).toBe('1 reconnecting... · 1 disconnected');
  });
  it.each(['NATIVE_UNAVAILABLE', 'AUTH_FAILED', 'APPROVAL_REQUIRED', 'ACCESS_DENIED'] as const)('routes %s into live state instead of a sticky notice', code => {
    expect(isConnectionFailure({ code, message: 'Connection changed', retryable: true })).toBe(true);
  });
  it.each(['FOCUS_DENIED', 'STORAGE_FAILED', 'VALIDATION', 'NOT_FOUND'] as const)('does not hide actionable %s errors', code => {
    expect(isConnectionFailure({ code, message: 'Action failed', retryable: false })).toBe(false);
  });
});
