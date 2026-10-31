import { afterEach, expect, it, vi } from 'vitest';
import { NetworkRecovery, NETWORK_RECOVERY_GRACE_MS } from './network-recovery';
afterEach(() => vi.useRealTimers());
it('keeps transient failures quiet, exposes persistent blocking failures and resets on recovery', () => {
  vi.useFakeTimers();
  const recovery = new NetworkRecovery();
  expect(recovery.failure('Unavailable', false)).toEqual({ status: 'starting', loginUrl: null, error: null });
  vi.advanceTimersByTime(NETWORK_RECOVERY_GRACE_MS);
  expect(recovery.failure('Still unavailable', false)).toMatchObject({ status: 'error', error: 'Still unavailable' });
  recovery.reset();
  expect(recovery.pending).toBe(false);
  expect(recovery.failure('New resume', false).error).toBeNull();
});
it('does not report control health failures while an authenticated connection still works', () => {
  vi.useFakeTimers();
  const recovery = new NetworkRecovery();
  recovery.failure('Control unavailable', true);
  vi.advanceTimersByTime(NETWORK_RECOVERY_GRACE_MS * 2);
  expect(recovery.failure('Control unavailable', true).error).toBeNull();
  expect(recovery.failure('Control unavailable', false).status).toBe('error');
});
