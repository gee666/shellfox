import { it, expect, vi } from 'vitest';
import { closeGuest, GUEST_CLOSE_SCRIPT } from './guest-close';
import type { TrackingRoot } from './tracking';
const root: TrackingRoot = { tabId: 'tab', sessionId: 'session', generation: 'generation', pid: 888, environment: 'wsl', distro: "Ubuntu';&", marker: 'private-marker' };
it('uses guest exact birth and pidfd instead of killing the host transport or numeric PID alone', async () => {
  const run = vi.fn(async () => Buffer.from('')), identity = { pid: 123, birth: '12345678-1234-1234-1234-123456789abc:456' };
  await closeGuest(root, identity, run);
  expect(run).toHaveBeenCalledWith(expect.stringMatching(/wsl\.exe$/), ['--distribution', root.distro, '--exec', 'python3', '-c', GUEST_CLOSE_SCRIPT, '123', identity.birth, 'private-marker', '[]']);
  expect(GUEST_CLOSE_SCRIPT).toContain('os.pidfd_open'); expect(GUEST_CLOSE_SCRIPT).toContain('signal.pidfd_send_signal'); expect(GUEST_CLOSE_SCRIPT).toContain("r['ticks'] != birth");
});
it('refuses unknown or malformed guest identities without invoking a command', async () => {
  const run = vi.fn(async () => Buffer.from('')); await expect(closeGuest(root, null, run)).rejects.toThrow(); await expect(closeGuest(root, { pid: 123, birth: 'unknown' }, run)).rejects.toThrow(); expect(run).not.toHaveBeenCalled();
});
it('propagates unavailable guest Python/pidfd or changed identity as a termination refusal', async () => {
  await expect(closeGuest(root, { pid: 123, birth: '12345678-1234-1234-1234-123456789abc:456' }, async () => { throw new Error('guest inspection unavailable'); })).rejects.toThrow();
});
