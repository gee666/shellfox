import { expect, it, vi } from 'vitest';
import { REMOTE_APPROVAL_TTL_MS, RemoteApprovals } from './approvals';

const peer = { id: 'peer', hostname: 'peer' };
it('withdraws only the requested attempt and leaves approved device permission intact', () => {
  let allowed = false; const changed = vi.fn(), policy = new RemoteApprovals(() => allowed, changed);
  policy.authorize(peer, 'one'); policy.authorize(peer, 'two'); policy.withdraw(peer.id, 'one');
  expect(policy.pending.size).toBe(1); expect(policy.authorize(peer, 'one')?.code).toBe('ACCESS_DENIED');
  policy.withdraw(peer.id, 'two'); expect(policy.pending.size).toBe(0);
  expect(policy.authorize(peer, 'three')?.code).toBe('APPROVAL_REQUIRED');
  allowed = true; policy.approve(peer.id); policy.withdraw(peer.id, 'three');
  expect(policy.authorize(peer, 'three')?.code).toBe('ACCESS_DENIED');
  expect(policy.authorize(peer, 'fresh')).toBeNull(); expect(policy.pending.size).toBe(0);
});
it('expires unreachable pending attempts without expiring a reachable attempt on the same device', () => {
  let now = 0; const policy = new RemoteApprovals(() => false, () => {}, () => now);
  policy.authorize(peer, 'unreachable'); policy.authorize(peer, 'reachable');
  now = REMOTE_APPROVAL_TTL_MS - 20_000; policy.authorize(peer, 'reachable');
  now = REMOTE_APPROVAL_TTL_MS + 10_000; policy.expire(); expect(policy.pending.size).toBe(1);
  expect(policy.authorize(peer, 'unreachable')?.code).toBe('APPROVAL_REQUIRED');
  policy.withdraw(peer.id, 'reachable'); expect(policy.pending.size).toBe(1);
  policy.withdraw(peer.id, 'unreachable'); expect(policy.pending.size).toBe(0);
  expect(policy.authorize(peer, 'fresh')?.code).toBe('APPROVAL_REQUIRED');
  now += REMOTE_APPROVAL_TTL_MS + 1; policy.expire(); expect(policy.pending.size).toBe(0);
});
it('never reports waiting when the approval queue cannot admit the request', () => {
  let now = 0; const changed = vi.fn(), policy = new RemoteApprovals(() => false, changed, () => now);
  for (let i = 0; i < 100; i++) expect(policy.authorize({ id: `peer-${i}`, hostname: 'peer' }, 'attempt')?.code).toBe('APPROVAL_REQUIRED');
  expect(policy.authorize(peer, 'same-active-attempt')).toMatchObject({ code: 'VALIDATION', retryable: true });
  expect(policy.pending.has(peer.id)).toBe(false); expect(changed).toHaveBeenCalledTimes(100);
  // Resolve one slot. Retrying the same active UUID can now create the host request.
  policy.withdraw('peer-0', 'attempt');
  expect(policy.authorize(peer, 'same-active-attempt')?.code).toBe('APPROVAL_REQUIRED');
  expect(policy.pending.has(peer.id)).toBe(true);
  now = REMOTE_APPROVAL_TTL_MS + 1;
  expect(policy.authorize({ id: 'new-peer', hostname: 'peer' }, 'new-attempt')?.code).toBe('APPROVAL_REQUIRED');
  expect([...policy.pending.keys()]).toEqual(['new-peer']);
});
it('keeps active pending requests through unrelated attempt churn and late cleanup', () => {
  const changed = vi.fn(), policy = new RemoteApprovals(() => false, changed);
  policy.authorize(peer, 'old'); policy.authorize(peer, 'fresh');
  for (let i = 0; i < 1100; i++) policy.withdraw(`unrelated-${i}`, 'cancelled');
  expect(policy.pending.has(peer.id)).toBe(true);
  policy.withdraw(peer.id, 'old');
  expect(policy.pending.has(peer.id)).toBe(true);
  expect(policy.authorize(peer, 'fresh')?.code).toBe('APPROVAL_REQUIRED');
  expect(policy.authorize(peer, 'old')?.code).toBe('ACCESS_DENIED');
  expect(changed).toHaveBeenCalledOnce();
});
it('lets an expired active attempt retry a lost approval response without treating silence as rejection', () => {
  let now = 0; const policy = new RemoteApprovals(() => false, () => {}, () => now);
  policy.authorize(peer, 'active'); now = 180_000; policy.expire();
  expect(policy.pending.has(peer.id)).toBe(true);
  now = REMOTE_APPROVAL_TTL_MS + 1; policy.expire();
  expect(policy.pending.size).toBe(0);
  expect(policy.authorize(peer, 'active')?.code).toBe('APPROVAL_REQUIRED');
  expect(policy.pending.has(peer.id)).toBe(true);
  policy.withdraw(peer.id, 'active');
  expect(policy.authorize(peer, 'active')?.code).toBe('ACCESS_DENIED');
});
it('does not let a restarted process cancel an unknown earlier attempt for the same peer', () => {
  let now = 0; const policy = new RemoteApprovals(() => false, () => {}, () => now);
  policy.authorize(peer, 'old-process'); now = 180_000;
  policy.authorize(peer, 'new-process'); policy.withdraw(peer.id, 'new-process');
  expect(policy.pending.has(peer.id)).toBe(true);
  expect(policy.authorize(peer, 'new-process')?.code).toBe('ACCESS_DENIED');
  now = REMOTE_APPROVAL_TTL_MS + 1; policy.expire();
  expect(policy.pending.size).toBe(0);
});
it('bounds tombstones with LRU and TTL instead of denying every future peer or attempt at capacity', () => {
  let now = 0; const policy = new RemoteApprovals(() => false, () => {}, () => now);
  for (let i = 0; i < 300; i++) policy.withdraw(peer.id, 'attempt-' + i);
  expect(policy.authorize(peer, 'attempt-299')?.code).toBe('ACCESS_DENIED');
  expect(policy.authorize(peer, 'attempt-0')?.code).toBe('APPROVAL_REQUIRED');
  for (let i = 0; i < 1100; i++) policy.withdraw('peer-' + i, 'attempt');
  expect(policy.authorize({ id: 'brand-new', hostname: 'new' }, 'attempt')?.code).toBe('APPROVAL_REQUIRED');
  expect(policy.authorize({ id: 'peer-0', hostname: 'old' }, 'attempt')?.code).toBe('APPROVAL_REQUIRED');
  policy.withdraw('ttl-peer', 'old'); now = 300_001; policy.expire();
  expect(policy.authorize({ id: 'ttl-peer', hostname: 'ttl' }, 'old')?.code).toBe('APPROVAL_REQUIRED');
});
