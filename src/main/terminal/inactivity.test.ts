import { randomUUID } from 'node:crypto';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Result, SessionDto } from '../../shared/contracts';
import type { TabObservation } from '../../shared/native-port';
import { PtyBackend } from './backend';
import { EmbeddedSessionService } from './service';
import { factoryFixture, MemoryRepository } from './test-fixtures';

const HOUR = 3_600_000, MINUTE = 60_000;
const value = <T>(result: Result<T>): T => { if (!result.ok) throw new Error(result.error.message); return result.value; };
afterEach(() => { vi.useRealTimers(); });
async function fixture(withPolling = false) {
  vi.useFakeTimers({ toFake: ['Date', 'setInterval', 'clearInterval'] });
  vi.setSystemTime(new Date('2026-01-01T00:00:00Z'));
  const f = factoryFixture(), repository = new MemoryRepository();
  repository.config.sessionSettleHours = repository.config.sessionDeleteHours = 1;
  let emit!: (items: TabObservation[]) => void;
  const descendants = vi.fn(() => [{ domain: 'host', pid: 500, birth: 'first' }]);
  const poll = vi.fn(async () => {}), setVisibleSessions = vi.fn();
  const backend = new PtyBackend(f.options);
  const service = new EmbeddedSessionService(repository, backend, callback => {
    emit = callback;
    return { setWatch() {}, dispose() {}, resolveDescendants: descendants, setVisibleSessions, ...(withPolling ? { poll } : {}) };
  });
  await service.initialize();
  const session = value(await service.createSession({ cwd: '/work', requestId: randomUUID() }));
  const observe = (agents = 1, health: 'healthy' | 'unknown' = 'healthy') => emit([{ sessionId: session.id, tabId: session.tabs[0].id, observedAt: new Date().toISOString(), root: 'alive', health, agents, reason: null }]);
  const jump = async (duration: number) => { vi.setSystemTime(Date.now() + duration - MINUTE); await vi.advanceTimersByTimeAsync(MINUTE); };
  const current = (): SessionDto => value(service.getSnapshot()).sessions.find(s => s.id === session.id)!;
  return { ...f, repository, service, session, backend, observe, descendants, poll, setVisibleSessions, jump, current };
}

describe('session inactivity', () => {
  it('refreshes hidden roots only at expiry and rechecks silent process activity before settling or deleting', async () => {
    const f = await fixture(true);
    try {
      f.observe(0); f.poll.mockImplementation(async () => { f.observe(0); });
      value(f.service.setVisibleSessions({ sessionIds: [] }));
      await f.jump(HOUR - MINUTE); expect(f.poll).not.toHaveBeenCalled();
      f.descendants.mockReturnValue([{ domain: 'host', pid: 700, birth: 'silent-child' }]);
      await f.jump(MINUTE);
      expect(f.poll).toHaveBeenCalledTimes(1); expect(f.current().settledAt).toBeNull();
      expect(f.setVisibleSessions).toHaveBeenLastCalledWith([]);
      await f.jump(MINUTE); expect(f.poll).toHaveBeenCalledTimes(1);
      await f.jump(HOUR - MINUTE); expect(f.current().status).toBe('settled');
      expect(f.poll).toHaveBeenCalledTimes(2); expect(f.processes[0].kill).not.toHaveBeenCalled();
      f.descendants.mockReturnValue([{ domain: 'host', pid: 701, birth: 'another-silent-child' }]);
      await f.jump(HOUR); expect(f.current()).toBeDefined(); expect(f.poll).toHaveBeenCalledTimes(3);
      expect(f.processes[0].kill).not.toHaveBeenCalled();
      await f.jump(HOUR); expect(f.current()).toBeUndefined(); expect(f.poll).toHaveBeenCalledTimes(4);
      expect(f.processes[0].kill).toHaveBeenCalledTimes(1);
    } finally { await f.service.dispose(); }
  });
  it('defers automatic deletion on failed, stale or incomplete process evidence', async () => {
    const f = await fixture(true);
    try {
      f.observe(0); value(await f.service.settleSession({ sessionId: f.session.id, confirmActive: false }));
      f.poll.mockRejectedValueOnce(new Error('snapshot unavailable'));
      await f.jump(HOUR); expect(f.current()).toBeDefined();
      await f.jump(HOUR); expect(f.current()).toBeDefined(); // Resolved poll with no fresh observation.
      f.poll.mockImplementation(async () => { f.observe(0, 'unknown'); });
      await f.jump(HOUR); await f.jump(HOUR); expect(f.current()).toBeDefined();
      expect(f.processes[0].kill).not.toHaveBeenCalled();
      f.poll.mockImplementation(async () => { f.observe(0); });
      await f.jump(MINUTE); expect(f.current()).toBeDefined();
      await f.jump(HOUR); expect(f.current()).toBeUndefined();
    } finally { await f.service.dispose(); }
  });
  it('rechecks input arriving while the expiry poll is in flight', async () => {
    const f = await fixture(true);
    try {
      f.observe(0); value(await f.service.settleSession({ sessionId: f.session.id, confirmActive: false }));
      f.poll.mockImplementation(async () => {
        f.observe(0);
        value(f.service.writeTerminal({ tabId: f.session.tabs[0].id, generation: f.session.tabs[0].generation!, data: 'x' }));
      });
      await f.jump(HOUR); expect(f.current()).toBeDefined(); expect(f.processes[0].kill).not.toHaveBeenCalled();
      expect(f.repository.session(f.session.id)?.updatedAt).toBe(new Date().toISOString());
    } finally { await f.service.dispose(); }
  });
  it('settles without closing, then deletes after a separate grace period', async () => {
    const f = await fixture();
    try {
      await f.jump(HOUR - MINUTE); expect(f.current().settledAt).toBeNull();
      await f.jump(MINUTE); expect(f.current().status).toBe('settled');
      expect(f.current().tabs).toHaveLength(1); expect(f.processes[0].kill).not.toHaveBeenCalled();
      await f.jump(HOUR - MINUTE); expect(f.current()).toBeDefined();
      await f.jump(MINUTE); expect(f.current()).toBeUndefined();
      expect(f.processes[0].kill).toHaveBeenCalledTimes(1);
      expect(f.repository.operationMap.size).toBe(0);
    } finally { await f.service.dispose(); }
  });
  it('uses configured hours and never automatically deletes pinned sessions', async () => {
    const f = await fixture();
    try {
      f.repository.config.sessionSettleHours = 2; f.repository.config.sessionDeleteHours = 3;
      value(await f.service.setSessionPinned({ sessionId: f.session.id, pinned: true }));
      await f.jump(HOUR); expect(f.current().settledAt).toBeNull();
      await f.jump(HOUR); expect(f.current().status).toBe('settled');
      await f.jump(10 * HOUR); expect(f.current()).toBeDefined(); expect(f.backend.live()).toHaveLength(1);
      value(await f.service.setSessionPinned({ sessionId: f.session.id, pinned: false }));
      await f.jump(3 * HOUR - MINUTE); expect(f.current()).toBeDefined();
      await f.jump(MINUTE); expect(f.current()).toBeUndefined();
    } finally { await f.service.dispose(); }
  });
  it.each(['input', 'output', 'process'] as const)('%s delays settlement and writes activity at most once per sweep', async kind => {
    const f = await fixture();
    try {
      const save = vi.spyOn(f.repository, 'saveSession');
      await f.jump(HOUR - MINUTE);
      for (let i = 0; i < 20; i++) {
        if (kind === 'input') value(f.service.writeTerminal({ tabId: f.session.tabs[0].id, generation: f.session.tabs[0].generation!, data: 'x' }));
        else if (kind === 'output') f.processes[0].output('work');
        else f.observe(i);
      }
      expect(save).not.toHaveBeenCalled();
      await f.jump(MINUTE); expect(save).toHaveBeenCalledTimes(1); expect(f.current().settledAt).toBeNull();
      await f.jump(HOUR - MINUTE); expect(f.current().status).toBe('settled');
    } finally { await f.service.dispose(); }
  });
  it('ignores repeated poll timestamps but detects replacement processes with the same count', async () => {
    const f = await fixture();
    try {
      f.observe(); await f.jump(MINUTE);
      await f.jump(HOUR - 2 * MINUTE); f.observe();
      f.descendants.mockReturnValue([{ domain: 'host', pid: 500, birth: 'replacement' }]); f.observe();
      await f.jump(MINUTE); expect(f.current().settledAt).toBeNull();
      await f.jump(HOUR - MINUTE); f.observe();
      await f.jump(MINUTE); expect(f.current().status).toBe('settled');
    } finally { await f.service.dispose(); }
  });
  it('keeps settlement while new output and explicit actions restart the deletion clock', async () => {
    const f = await fixture();
    try {
      value(await f.service.settleSession({ sessionId: f.session.id, confirmActive: false }));
      await f.jump(HOUR - MINUTE); f.processes[0].output('still working');
      await f.jump(MINUTE); expect(f.current().status).toBe('settled');
      await f.jump(HOUR - 2 * MINUTE); value(await f.service.renameSession({ sessionId: f.session.id, title: 'Keep this' }));
      await f.jump(MINUTE); expect(f.current()).toBeDefined();
      await f.jump(HOUR - MINUTE); expect(f.current()).toBeUndefined();
    } finally { await f.service.dispose(); }
  });
  it('retries deletion after a failed owned-terminal close without losing ownership', async () => {
    const f = await fixture();
    try {
      value(await f.service.settleSession({ sessionId: f.session.id, confirmActive: false }));
      f.processes[0].kill.mockImplementation(() => { throw new Error('denied'); });
      await f.jump(HOUR); expect(f.current()).toBeDefined(); expect(f.backend.live()).toHaveLength(1);
      f.processes[0].kill.mockImplementation(() => f.processes[0].exit(0));
      await f.jump(MINUTE); expect(f.current()).toBeUndefined();
    } finally { f.processes[0].kill.mockImplementation(() => f.processes[0].exit(0)); await f.service.dispose(); }
  });
  it('does not settle recently used sessions after a short restart', async () => {
    const f = await fixture();
    vi.setSystemTime(Date.now() + HOUR / 2);
    f.processes[0].output('recent work');
    await f.service.dispose();
    const lastActivity = f.repository.session(f.session.id)?.updatedAt;
    vi.setSystemTime(Date.now() + 5 * MINUTE);
    const next = new EmbeddedSessionService(f.repository, new PtyBackend(f.options), () => ({ setWatch() {}, dispose() {} }));
    try {
      await next.initialize();
      await vi.advanceTimersByTimeAsync(MINUTE);
      expect(value(next.getSnapshot()).sessions[0]).toMatchObject({ id: f.session.id, settledAt: null, updatedAt: lastActivity, tabs: [] });
    } finally { await next.dispose(); }
  });
  it('persists the last activity on shutdown and grants newly settled old sessions a full delete window', async () => {
    const f = await fixture();
    vi.setSystemTime(Date.now() + HOUR / 2); f.processes[0].output('work');
    await f.service.dispose(); expect(f.repository.session(f.session.id)?.updatedAt).toBe(new Date().toISOString());
    const next = new EmbeddedSessionService(f.repository, new PtyBackend(f.options), () => ({ setWatch() {}, dispose() {} }));
    try {
      vi.setSystemTime(Date.now() + 10 * HOUR); await next.initialize();
      await vi.advanceTimersByTimeAsync(MINUTE);
      const restored = value(next.getSnapshot()).sessions[0];
      expect(restored.status).toBe('settled'); expect(restored.tabs).toEqual([]);
      await vi.advanceTimersByTimeAsync(MINUTE); expect(value(next.getSnapshot()).sessions).toHaveLength(1);
    } finally { await next.dispose(); }
  });
});
