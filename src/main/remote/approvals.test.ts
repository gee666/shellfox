import { expect, it, vi } from 'vitest';
import { RemoteApprovals } from './approvals';

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
  now = 100_000; policy.authorize(peer, 'reachable');
  now = 130_000; policy.expire(); expect(policy.pending.size).toBe(1);
  expect(policy.authorize(peer, 'unreachable')?.code).toBe('ACCESS_DENIED');
  policy.withdraw(peer.id, 'reachable'); expect(policy.pending.size).toBe(0);
  expect(policy.authorize(peer, 'fresh')?.code).toBe('APPROVAL_REQUIRED');
  now = 260_001; policy.expire(); expect(policy.pending.size).toBe(0);
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
