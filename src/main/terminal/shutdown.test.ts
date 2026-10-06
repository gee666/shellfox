import { it, expect, vi } from 'vitest';
import { TerminalQuitGuard } from './shutdown';
it('failed termination does not poison later quit retries or report success', async () => {
  const service = { ownedTerminalCount: () => 1, dispose: vi.fn().mockRejectedValueOnce(new Error('kill failed')).mockResolvedValueOnce(undefined) }, confirm = vi.fn(() => true), guard = new TerminalQuitGuard(service, confirm);
  await expect(guard.request()).rejects.toThrow('kill failed'); expect(await guard.request()).toBe(true); expect(service.dispose).toHaveBeenCalledTimes(2); expect(confirm).toHaveBeenCalledTimes(2);
});
it('native quit cancellation leaves owned shells running', async () => {
  const service = { ownedTerminalCount: () => 2, dispose: vi.fn(async () => {}) }, confirm = vi.fn(() => false);
  const guard = new TerminalQuitGuard(service, confirm); expect(await guard.request()).toBe(false); expect(confirm).toHaveBeenCalledWith(2); expect(service.dispose).not.toHaveBeenCalled();
});
it('acceptance closes owned shells exactly once across concurrent quit requests', async () => {
  const service = { ownedTerminalCount: () => 3, dispose: vi.fn(async () => {}) }, confirm = vi.fn(() => true);
  const guard = new TerminalQuitGuard(service, confirm); expect(await Promise.all([guard.request(), guard.request()])).toEqual([true, true]); expect(service.dispose).toHaveBeenCalledTimes(1); expect(confirm).toHaveBeenCalledTimes(1);
});
it('does not ask for shell-close confirmation when no owned shell or launch exists', async () => {
  const service = { ownedTerminalCount: () => 0, dispose: vi.fn(async () => {}) }, confirm = vi.fn();
  expect(await new TerminalQuitGuard(service, confirm).request()).toBe(true); expect(confirm).not.toHaveBeenCalled(); expect(service.dispose).toHaveBeenCalledTimes(1);
});
it('a cancelled request can later be accepted', async () => {
  const service = { ownedTerminalCount: () => 1, dispose: vi.fn(async () => {}) }, confirm = vi.fn().mockReturnValueOnce(false).mockReturnValueOnce(true), guard = new TerminalQuitGuard(service, confirm);
  expect(await guard.request()).toBe(false); expect(await guard.request()).toBe(true); expect(service.dispose).toHaveBeenCalledTimes(1);
});
