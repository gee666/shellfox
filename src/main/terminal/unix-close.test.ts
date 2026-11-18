import { it, expect } from 'vitest';
import { compactOwnedIdentities, UNIX_TREE_CLOSE_SCRIPT } from './unix-close';
const birth = '12345678-1234-1234-1234-123456789abc:123';
it('bounds and deduplicates previously proven descendant identities by boot, never by PID alone', () => {
  expect(compactOwnedIdentities({ pid: 1, birth }, [{ pid: 2, birth }, { pid: 2, birth }, { pid: 3, birth: 'foreign-boot:123' }])).toEqual([[2, '123']]);
  expect(() => compactOwnedIdentities({ pid: 1, birth }, Array.from({ length: 257 }, (_, i) => ({ pid: i + 1, birth })))).toThrow();
});
it('tree termination holds pidfds, stops to a fixed point, rolls back refusals and checks each exit', () => {
  expect(UNIX_TREE_CLOSE_SCRIPT).toContain('os.pidfd_open'); expect(UNIX_TREE_CLOSE_SCRIPT).toContain('signal.pidfd_send_signal');
  expect(UNIX_TREE_CLOSE_SCRIPT).toContain('signal.SIGSTOP'); expect(UNIX_TREE_CLOSE_SCRIPT).toContain('signal.SIGCONT'); expect(UNIX_TREE_CLOSE_SCRIPT).toContain('owned descendant exit not confirmed');
  expect(UNIX_TREE_CLOSE_SCRIPT).not.toContain('os.kill('); expect(UNIX_TREE_CLOSE_SCRIPT).not.toContain('os.killpg(');
  expect(UNIX_TREE_CLOSE_SCRIPT).toContain('owned session anchor was lost'); expect(UNIX_TREE_CLOSE_SCRIPT).toContain('signal.setitimer');
});
