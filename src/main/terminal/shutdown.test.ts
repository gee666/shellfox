import { it, expect, vi } from 'vitest';
import { TerminalQuitGuard } from './shutdown';
it('failed termination does not poison later quit retries or report success', async () => {
  const service = { ownedTerminalCount: () => 1, dispose: vi.fn().mockRejectedValueOnce(new Error('kill failed')).mockResolvedValueOnce(undefined) }, confirm = vi.fn(() => true), guard = new TerminalQuitGuard(service, confirm);
  await expect(guard.request()).rejects.toThrow('kill failed'); expect(await guard.request()).toBe(true); expect(service.dispose).toHaveBeenCalledTimes(2); expect(confirm).toHaveBeenCalledTimes(2);
});
it('quit cancellation leaves owned shells running', async () => {
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
it('shares asynchronous approval and does not dispose shells before a response', async () => {
  let answer!: (value: boolean) => void;
  const service = { ownedTerminalCount: () => 2, dispose: vi.fn(async () => {}) };
  const confirm = vi.fn(() => new Promise<boolean>(resolve => { answer = resolve; }));
  const guard = new TerminalQuitGuard(service, confirm);
  const first = guard.request(), repeated = guard.request();
  await Promise.resolve();
  expect(repeated).toBe(first); expect(service.dispose).not.toHaveBeenCalled();
  answer(false); expect(await first).toBe(false);
  const next = guard.request(); await Promise.resolve();
  answer(true); expect(await next).toBe(true);
  expect(confirm).toHaveBeenCalledTimes(2); expect(service.dispose).toHaveBeenCalledOnce();
});
it('a cancelled request can later be accepted', async () => {
  const service = { ownedTerminalCount: () => 1, dispose: vi.fn(async () => {}) }, confirm = vi.fn().mockReturnValueOnce(false).mockReturnValueOnce(true), guard = new TerminalQuitGuard(service, confirm);
  expect(await guard.request()).toBe(false); expect(await guard.request()).toBe(true); expect(service.dispose).toHaveBeenCalledTimes(1);
});
