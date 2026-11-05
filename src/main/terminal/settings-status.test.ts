import { randomUUID } from 'node:crypto';
import { afterEach, expect, it, vi } from 'vitest';
import type { Result, SettingsDto } from '../../shared/contracts';
import { sessionDot } from '../../shared/terminal-status';
import { settingsSchema } from '../../shared/schemas';
import { PtyBackend } from './backend';
import { EmbeddedSessionService } from './service';
import { ProcessTracker, type SnapshotProvider, type TrackingProcess } from './tracking';
import { factoryFixture, MemoryRepository } from './test-fixtures';

const cleanups: Array<() => Promise<void>> = [];
const value = <T>(result: Result<T>): T => { if (!result.ok) throw new Error(result.error.message); return result.value; };
afterEach(async () => {
  vi.restoreAllMocks();
  for (const close of cleanups.splice(0).reverse()) await close();
  vi.useRealTimers();
});

async function fixture() {
  vi.useFakeTimers();
  const f = factoryFixture(), repository = new MemoryRepository(), backend = new PtyBackend(f.options);
  repository.config = settingsSchema.parse({ ...repository.config, processRules: [{ id: randomUUID(), label: 'Agent', enabled: true, executablePaths: [], executableBasenames: ['node'], scriptPathSuffixes: [] }] });
  const provider = vi.fn<SnapshotProvider>(async roots => {
    const processes: TrackingProcess[] = [{ pid: process.pid, parentPid: 0, birth: 'manager', birthOrder: '1', executable: '/manager', argv: [], accessible: true }];
    for (const root of roots) {
      const birthOrder = root.birthOrderBounds?.min ?? '100';
      processes.push({ pid: root.pid, parentPid: root.ancestorPid ?? process.pid, birth: root.generation, birthOrder, executable: root.shellExecutable ?? '/bin/bash', argv: ['bash'], accessible: true, marker: root.marker });
      processes.push({ pid: root.pid + 1000, parentPid: root.pid, birth: `agent:${root.pid}`, birthOrder: (BigInt(birthOrder) + 10n).toString(), executable: '/usr/bin/node', argv: ['node'], accessible: true });
    }
    return [{ domain: 'test/local', environment: 'local', distro: null, complete: true, processes }];
  });
  let tracker!: ProcessTracker;
  const service = new EmbeddedSessionService(repository, backend, emit => (tracker = new ProcessTracker(emit, provider)));
  await service.initialize(); cleanups.push(() => service.dispose());
  const a = value(await service.createSession({ cwd: '/work', requestId: randomUUID() }));
  const b = value(await service.createSession({ cwd: '/work', requestId: randomUUID() }));
  await tracker.poll();
  const busy = { [a.tabs[0].id]: true };
  const sessions = () => value(service.getSnapshot()).sessions;
  const kinds = () => [a, b].map(created => sessionDot(sessions().find(session => session.id === created.id)!, busy).kind);
  expect(kinds()).toEqual(['busy', 'running']);
  value(service.setVisibleSessions({ sessionIds: [a.id] }));
  await vi.advanceTimersByTimeAsync(0);
  return { ...f, repository, service, tracker, provider, a, b, sessions, kinds };
}

it('preserves visible and hidden agent status and busy dots across unrelated settings autosaves', async () => {
  const f = await fixture(), setWatch = vi.spyOn(f.tracker, 'setWatch');
  const published: string[][] = [];
  f.service.subscribe(() => published.push(f.kinds()));
  const changes: Array<Partial<SettingsDto>> = [
    {}, { accentColor: '#123456' }, { backgroundColor: '#112233' }, { historyPageSize: 7 },
    { sessionSettleHours: 48 }, { sessionDeleteHours: 1440 }, { terminalProfileId: 'wsl:Ubuntu' },
    { terminalEnv: [{ name: 'EXAMPLE', value: 'value' }] },
  ];
  const polls = f.provider.mock.calls.length;
  for (const change of changes) {
    value(await f.service.saveSettings({ ...f.repository.settings(), ...change }));
    expect(f.kinds()).toEqual(['busy', 'running']);
    expect(f.sessions().map(session => session.counts.agents)).toEqual([1, 1]);
  }
  expect(setWatch).not.toHaveBeenCalled();
  expect(f.provider).toHaveBeenCalledTimes(polls);
  await vi.advanceTimersByTimeAsync(2000);
  expect(f.provider.mock.lastCall![0].map(root => root.sessionId)).toEqual([f.a.id]);
  expect(f.kinds()).toEqual(['busy', 'running']);
  expect(published.length).toBeGreaterThan(0);
  expect(published.every(kinds => kinds.join() === 'busy,running')).toBe(true);
});

it('retains status after a failed unrelated settings save', async () => {
  const f = await fixture(), previous = f.repository.settings(), setWatch = vi.spyOn(f.tracker, 'setWatch');
  vi.spyOn(f.repository, 'saveSettings').mockImplementationOnce(() => { throw new Error('disk full'); });
  expect(await f.service.saveSettings({ ...previous, accentColor: '#123456' })).toMatchObject({ ok: false, error: { code: 'STORAGE_FAILED' } });
  expect(f.repository.settings()).toEqual(previous);
  expect(f.kinds()).toEqual(['busy', 'running']);
  expect(setWatch).not.toHaveBeenCalled();
});

it('restores old rules and keeps visible and hidden evidence when rule persistence fails', async () => {
  const f = await fixture(), previous = f.repository.settings(), setWatch = vi.spyOn(f.tracker, 'setWatch');
  vi.spyOn(f.repository, 'saveSettings').mockImplementationOnce(() => { throw new Error('disk full'); });
  expect(await f.service.saveSettings({ ...previous, processRules: [] })).toMatchObject({ ok: false, error: { code: 'STORAGE_FAILED' } });
  expect(f.repository.settings()).toEqual(previous);
  expect(setWatch).toHaveBeenCalledTimes(2);
  expect(setWatch.mock.lastCall![1]).toEqual(previous.processRules);
  expect(f.kinds()).toEqual(['busy', 'running']);
  await vi.advanceTimersByTimeAsync(0);
  expect(f.kinds()).toEqual(['busy', 'running']);
  expect(f.sessions().map(session => session.counts.agents)).toEqual([1, 1]);
});

it('keeps evidence when the tracker rejects a rule update', async () => {
  const f = await fixture(), previous = f.repository.settings();
  vi.spyOn(f.tracker, 'setWatch').mockImplementationOnce(() => { throw new Error('watch rejected'); });
  expect(await f.service.saveSettings({ ...previous, processRules: [] })).toMatchObject({ ok: false, error: { code: 'STORAGE_FAILED' } });
  expect(f.repository.settings()).toEqual(previous);
  expect(f.kinds()).toEqual(['busy', 'running']);
});

it('still exposes real process inspection failures and recovers only from fresh evidence', async () => {
  const f = await fixture();
  f.provider.mockRejectedValueOnce(new Error('native inspection failed'));
  await f.tracker.poll();
  const unavailable = f.sessions();
  expect(unavailable.every(session => session.status === 'unknown' && session.counts.agents === 0)).toBe(true);
  value(await f.service.saveSettings({ ...f.repository.settings(), accentColor: '#123456' }));
  expect(f.sessions()).toEqual(unavailable);
  await f.tracker.poll();
  expect(f.kinds()).toEqual(['busy', 'running']);
});

it('invalidates old evidence only for a successful rule change and rejects an in-flight old-rule scan', async () => {
  const f = await fixture();
  const original = f.provider.getMockImplementation();
  if (!original) throw new Error('Missing snapshot provider');
  let deliver!: () => void;
  f.provider.mockImplementationOnce((roots, signal) => new Promise(resolve => {
    deliver = () => { void original(roots, signal).then(resolve); };
  }));
  const pending = f.tracker.poll();
  value(await f.service.saveSettings({ ...f.repository.settings(), processRules: [] }));
  expect(f.sessions().every(session => session.status === 'unknown' && session.counts.agents === 0)).toBe(true);
  deliver(); await pending;
  expect(f.sessions().every(session => session.status === 'waiting' && session.counts.agents === 0)).toBe(true);
  expect(f.kinds()).toEqual(['shell', 'shell']);
});
