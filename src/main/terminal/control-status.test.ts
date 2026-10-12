import { describe, expect, it, vi } from 'vitest';
import { randomUUID } from 'node:crypto';
import { EmbeddedSessionService } from './service';
import { PtyBackend } from './backend';
import { ProcessTracker, type TrackingProcess, type TrackingRoot } from './tracking';
import { MemoryRepository, factoryFixture } from './test-fixtures';
import { handleControlRequest } from '../control-server';
import { filterControl } from '../control-cli';
import type { Result } from '../../shared/contracts';

const value = <T>(r: Result<T>): T => { if (!r.ok) throw new Error(r.error.message); return r.value; };

describe('fresh CLI status independent of renderer viewport', () => {
  it('polls all owned roots before listing/filtering without changing automatic visibility', async () => {
    const repository = new MemoryRepository(), f = factoryFixture();
    repository.config.processRules = [{ id: randomUUID(), label: 'Agent', enabled: true, executableBasenames: ['agent.exe'], executablePaths: [], scriptPathSuffixes: [] }];
    const busy = new Set<string>();
    const provider = vi.fn(async (roots: readonly TrackingRoot[]) => {
      const processes: TrackingProcess[] = [{ pid: process.pid, parentPid: 0, birth: 'manager', birthOrder: '1', executable: '/manager.exe', argv: [], accessible: true }];
      for (const root of roots) {
        const birthOrder = root.birthOrderBounds?.min ?? '100';
        processes.push({ pid: root.pid, parentPid: root.ancestorPid ?? process.pid, birth: root.generation, birthOrder, executable: root.shellExecutable ?? '/bin/bash', argv: [], accessible: true, marker: root.marker });
        if (busy.has(root.sessionId)) processes.push({ pid: root.pid + 1000, parentPid: root.pid, birth: 'agent-' + root.generation, birthOrder: (BigInt(birthOrder) + 10n).toString(), executable: '/agents/agent.exe', argv: ['agent.exe'], accessible: true });
      }
      return [{ domain: 'host', environment: 'local' as const, distro: null, complete: true, processes }];
    });
    let tracker!: ProcessTracker;
    const service = new EmbeddedSessionService(repository, new PtyBackend(f.options), emit => tracker = new ProcessTracker(emit, provider));
    await service.initialize();
    try {
      const a = value(await service.createSession({ cwd: '/a', requestId: randomUUID() }));
      const b = value(await service.createSession({ cwd: '/b', requestId: randomUUID() }));
      value(await service.getControlSnapshot());
      value(service.setVisibleSessions({ sessionIds: [a.id] }));
      await expect.poll(() => provider.mock.calls.at(-1)?.[0].map(r => r.sessionId)).toEqual([a.id]);
      const visibility = vi.spyOn(tracker, 'setVisibleSessions');
      busy.add(b.id); // Off-screen session changed since its previous observation.
      expect(value(service.getSnapshot()).sessions.find(s => s.id === b.id)?.activityStatus).toBe('waiting');
      const listing = await handleControlRequest(service, { active: false, set: () => false }, { action: 'list' });
      expect(provider.mock.calls.at(-1)?.[0].map(r => r.sessionId).sort()).toEqual([a.id, b.id].sort());
      expect(visibility).not.toHaveBeenCalled();
      expect(filterControl(listing, { running: true }).sessions?.map(s => s.id)).toEqual([b.id]);
      expect(filterControl(listing, { idle: true }).sessions?.map(s => s.id)).toEqual([a.id]);
      busy.clear();
      const refreshed = await handleControlRequest(service, { active: false, set: () => false }, { action: 'list' });
      expect(filterControl(refreshed, { running: true }).sessions).toEqual([]);
      expect(filterControl(refreshed, { idle: true }).sessions).toHaveLength(2);
    } finally { await service.dispose(); }
  });
  it('fails rather than returning stale statuses if explicit polling fails', async () => {
    const repository = new MemoryRepository(), f = factoryFixture();
    const poll = vi.fn(async () => { throw new Error('Tracker failure'); });
    const service = new EmbeddedSessionService(repository, new PtyBackend(f.options), () => ({ setWatch() {}, dispose() {}, poll }));
    await service.initialize();
    try {
      expect(await service.getControlSnapshot()).toMatchObject({ ok: false, error: { code: 'MONITOR_UNAVAILABLE' } });
      await expect(handleControlRequest(service, { active: false, set: () => false }, { action: 'list' })).rejects.toThrow('refreshed');
    } finally { await service.dispose(); }
    poll.mockClear();
    expect(await service.getControlSnapshot()).toMatchObject({ ok: false, error: { code: 'NATIVE_UNAVAILABLE' } });
    expect(poll).not.toHaveBeenCalled();
  });
});
