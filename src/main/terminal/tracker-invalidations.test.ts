import { randomUUID } from 'node:crypto';
import { afterEach, expect, it, vi } from 'vitest';
import type { ChangedEvent, Result } from '../../shared/contracts';
import type { TabObservation } from '../../shared/native-port';
import { PtyBackend } from './backend';
import { EmbeddedSessionService } from './service';
import type { TrackingRoot } from './tracking';
import { factoryFixture, MemoryRepository } from './test-fixtures';
const MINUTE = 60_000, HOUR = 3_600_000;
const cleanups: Array<() => Promise<void>> = [];
const value = <T>(result: Result<T>): T => { if (!result.ok) throw new Error(result.error.message); return result.value; };
afterEach(async () => { for (const close of cleanups.splice(0).reverse()) await close(); vi.restoreAllMocks(); vi.useRealTimers(); });
async function fixture(withPolling = false) {
  vi.useFakeTimers({ toFake: ['Date', 'setInterval', 'clearInterval'] });
  vi.setSystemTime(new Date('2026-01-01T00:00:00Z'));
  const f = factoryFixture(), repository = new MemoryRepository(), backend = new PtyBackend(f.options);
  repository.config.sessionSettleHours = repository.config.sessionDeleteHours = 1;
  let emit: (items: TabObservation[]) => void = () => { throw new Error('Tracker not initialized'); };
  const poll = vi.fn(async () => {});
  const resolveIdentity = vi.fn((root: TrackingRoot) => ({ domain: 'host', pid: root.pid, birth: 'shell-birth' }));
  const resolveDescendants = vi.fn(() => [{ domain: 'host', pid: 500, birth: 'first-child' }]);
  const service = new EmbeddedSessionService(repository, backend, callback => {
    emit = callback;
    return { setWatch() {}, dispose() {}, resolveIdentity, resolveDescendants, ...(withPolling ? { poll } : {}) };
  });
  await service.initialize(); cleanups.push(() => service.dispose());
  const session = value(await service.createSession({ cwd: '/work', requestId: randomUUID() })), tab = session.tabs[0];
  const observation = (patch: Partial<TabObservation> = {}): TabObservation => ({ sessionId: session.id, tabId: tab.id, observedAt: new Date().toISOString(), root: 'alive', health: 'healthy', agents: 0, reason: null, ...patch });
  const observe = (patch?: Partial<TabObservation>) => emit([observation(patch)]);
  const events: ChangedEvent[] = []; service.subscribe(event => events.push(event));
  const native = () => events.filter(event => event.reason === 'native');
  return { ...f, repository, backend, service, session, tab, poll, resolveIdentity, resolveDescendants, observation, observe, emit, events, native };
}
it('publishes the first observation but 1000 timestamp-only polls cause no extra Changed/getSnapshot pressure', async () => {
  const f = await fixture(), snapshots = vi.spyOn(f.service, 'getSnapshot'), dto = vi.spyOn(f.service, 'toDto'), sessions = vi.spyOn(f.repository, 'sessions');
  const remember = vi.spyOn(f.backend, 'rememberOwnership');
  f.resolveIdentity.mockClear(); f.resolveDescendants.mockClear();
  // Model clients that fetch a snapshot in response to every invalidation.
  f.service.subscribe(() => { f.service.getSnapshot(); });
  f.observe(); expect(f.native()).toHaveLength(1); expect(snapshots).toHaveBeenCalledOnce();
  const at = Date.now();
  for (let index = 1; index <= 1000; index++) f.observe({ observedAt: new Date(at + index * 1000).toISOString() });
  expect(f.events).toHaveLength(1); expect(f.native()).toHaveLength(1);
  expect(snapshots).toHaveBeenCalledOnce(); expect(dto).toHaveBeenCalledOnce(); expect(sessions).toHaveBeenCalledOnce();
  expect(remember).toHaveBeenCalledTimes(1001); expect(f.resolveIdentity).toHaveBeenCalledTimes(1001);
  expect(f.resolveDescendants).toHaveBeenCalledTimes(2002);
});
it('pushes public agent, health, root and diagnostic transitions, once per changed batch', async () => {
  const f = await fixture();
  const transitions: Array<{ observation: Partial<TabObservation>; status: string; agents: number }> = [
    { observation: {}, status: 'waiting', agents: 0 },
    { observation: { agents: 1 }, status: 'running', agents: 1 },
    { observation: { agents: 2 }, status: 'running', agents: 2 },
    { observation: { agents: 0, health: 'unknown', reason: 'Incomplete snapshot' }, status: 'unknown', agents: 0 },
    { observation: { root: 'unavailable', health: 'unknown', reason: 'Root inaccessible' }, status: 'unknown', agents: 0 },
    { observation: { root: 'exited', health: 'unknown', reason: 'Root exited' }, status: 'unknown', agents: 0 },
    { observation: {}, status: 'waiting', agents: 0 },
  ];
  for (const [index, transition] of transitions.entries()) {
    f.observe(transition.observation); expect(f.native()).toHaveLength(index + 1);
    const current = value(f.service.getSnapshot()).sessions[0].tabs[0];
    expect(current).toMatchObject({ status: transition.status, agents: transition.agents });
    if (transition.observation.reason) expect(current.monitoringReason).toBe(transition.observation.reason);
  }
  f.emit([f.observation({ agents: 3 }), f.observation({ agents: 4 })]);
  expect(f.native()).toHaveLength(transitions.length + 1);
  expect(value(f.service.getSnapshot()).sessions[0].tabs[0].agents).toBe(4);
});
it('does not publish raw diagnostic changes when the public fields remain identical', async () => {
  const f = await fixture(); f.observe({ agents: 1 });
  const before = value(f.service.getSnapshot()).sessions[0].tabs[0];
  f.observe({ agents: 1, health: 'unknown' });
  f.observe({ agents: 1, health: 'unknown', reason: before.monitoringReason });
  const after = value(f.service.getSnapshot()).sessions[0].tabs[0];
  expect(after).toEqual(before); expect(f.native()).toHaveLength(1);
});
it('ignores empty/foreign/closed observations while new generations and lifecycle changes still invalidate', async () => {
  const f = await fixture(); f.observe();
  f.emit([]); f.observe({ sessionId: randomUUID() }); f.observe({ tabId: randomUUID() });
  expect(f.native()).toHaveLength(1);
  value(await f.service.closeTab({ tabId: f.tab.id, generation: f.tab.generation! }));
  const closedCount = f.native().length; expect(closedCount).toBeGreaterThan(1);
  f.observe(); expect(f.native()).toHaveLength(closedCount);
  const reopened = value(await f.service.activateSession({ sessionId: f.session.id })), next = reopened.tabs[0];
  expect(next.generation).not.toBe(f.tab.generation);
  f.observe({ tabId: next.id }); expect(f.native()).toHaveLength(closedCount + 1);
});
it('refreshes descendant ownership/activity evidence without a native invalidation when public status is unchanged', async () => {
  const f = await fixture(), remember = vi.spyOn(f.backend, 'rememberOwnership');
  f.observe();
  vi.setSystemTime(Date.now() + 29 * MINUTE); await vi.advanceTimersByTimeAsync(MINUTE);
  const activityAt = new Date().toISOString();
  f.resolveDescendants.mockReturnValue([{ domain: 'host', pid: 501, birth: 'replacement-child' }]);
  f.observe(); expect(f.native()).toHaveLength(1);
  expect(remember).toHaveBeenLastCalledWith(f.tab.id, f.tab.generation, expect.objectContaining({ birth: 'shell-birth' }), [{ domain: 'host', pid: 501, birth: 'replacement-child' }]);
  await vi.advanceTimersByTimeAsync(MINUTE);
  expect(f.repository.session(f.session.id)?.updatedAt).toBe(activityAt);
  expect(f.native()).toHaveLength(1);
});
it('keeps timestamp-only evidence fresh enough for expiry and still rejects stale cleanup evidence', async () => {
  const f = await fixture(true), originalTimestamp = new Date().toISOString();
  f.observe(); value(await f.service.settleSession({ sessionId: f.session.id, confirmActive: false }));
  f.poll.mockImplementation(async () => { f.observe({ observedAt: originalTimestamp }); });
  vi.setSystemTime(Date.now() + HOUR - MINUTE); await vi.advanceTimersByTimeAsync(MINUTE);
  expect(f.poll).toHaveBeenCalledOnce(); expect(f.repository.session(f.session.id)).toBeDefined();
  expect(f.processes[0].kill).not.toHaveBeenCalled(); expect(f.native()).toHaveLength(1);
  let nativeAfterFreshPoll = -1;
  f.poll.mockImplementation(async () => { f.observe(); nativeAfterFreshPoll = f.native().length; });
  await vi.advanceTimersByTimeAsync(MINUTE);
  expect(f.poll).toHaveBeenCalledTimes(2); expect(nativeAfterFreshPoll).toBe(1);
  expect(f.repository.session(f.session.id)).toBeUndefined(); expect(f.processes[0].kill).toHaveBeenCalledOnce();
});
