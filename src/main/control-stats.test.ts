import { describe, expect, it, vi } from 'vitest';
import type { OwnedTerminal } from './terminal/backend';
import type { TrackingProcess, TrackingSnapshot, SnapshotProvider } from './terminal/tracking';
import { collectSessionStats, createSessionStatsCollector } from './control-stats';
import { linuxElapsed, parseDarwinPs, parseLinuxMemory, parseLinuxStat, parseMetricRows, type MetricsProvider } from './control-stats-metrics';

const entry = (tabId = 'a', pid = 10, overrides: Partial<OwnedTerminal['root']> = {}): OwnedTerminal => ({
  tabId, sessionId: 'session-' + tabId, generation: 'gen', profileId: 'bash', cwd: '/', state: 'open', exitCode: null,
  cols: 80, rows: 24, cleanupPending: false, processTitle: null,
  root: { tabId, sessionId: 'session-' + tabId, generation: 'gen', pid, environment: 'local', distro: null, marker: 'marker-' + tabId, shellExecutable: '/bin/bash', ...overrides },
});
const proc = (pid: number, parentPid: number, overrides: Partial<TrackingProcess> = {}): TrackingProcess => ({
  pid, parentPid, birth: 'boot:' + pid, birthOrder: String(pid), accessible: true,
  executable: '/usr/bin/worker', processName: 'worker', argv: ['worker'], ...overrides,
});
const shell = (pid = 10, tab = 'a') => proc(pid, 1, { executable: '/bin/bash', processName: 'bash', marker: 'marker-' + tab });
const snapshot = (processes: TrackingProcess[], overrides: Partial<TrackingSnapshot> = {}): TrackingSnapshot => ({
  domain: 'host/local', environment: 'local', distro: null, processes, complete: true, rootMarkerRequired: true, ...overrides,
});
function harness(initial: TrackingSnapshot[]) {
  let snapshots = initial;
  const provider = vi.fn<SnapshotProvider>(async () => snapshots);
  const metrics = vi.fn<MetricsProvider>(async scope => scope.map(s => ({ domain: s.domain, processes: s.processes.map(p => ({
    pid: p.pid, parentPid: p.parentPid, birth: p.birth!, memoryBytes: p.pid * 1024, elapsedSeconds: p.pid === 10 ? 100 : 20,
  })) })));
  const collect = createSessionStatsCollector(provider, metrics);
  return { collect, provider, metrics, step: (next: TrackingSnapshot[]) => { snapshots = next; } };
}

describe('session stats ownership', () => {
  it('does no work for an empty live list', async () => {
    const h = harness([]);
    expect(await h.collect([])).toEqual([]);
    expect(h.provider).not.toHaveBeenCalled(); expect(h.metrics).not.toHaveBeenCalled();
    expect(await collectSessionStats([])).toEqual([]);
  });
  it('attributes only root and owned descendants, batching both tabs once', async () => {
    const h = harness([snapshot([shell(), proc(11, 10), proc(12, 11), shell(20, 'b'), proc(21, 20), proc(99, 1)])]);
    const stats = await h.collect([entry(), entry('b', 20)]);
    expect(stats[0]).toMatchObject({ tabId: 'a', memoryBytes: 33 * 1024, elapsedSeconds: 100 });
    expect(stats[0].processes.map(p => p.pid)).toEqual([10, 11, 12]);
    expect(stats[1].processes.map(p => p.pid)).toEqual([20, 21]);
    expect(stats[0].reason).toBeUndefined();
    expect(stats[0].processes[1]).toEqual({ pid: 11, parentPid: 10, name: 'worker', domain: 'host/local', memoryBytes: 11 * 1024, elapsedSeconds: 20 });
    expect(h.provider).toHaveBeenCalledTimes(1); expect(h.metrics).toHaveBeenCalledTimes(1);
    expect(h.metrics.mock.calls[0][0][0].processes.map(p => p.pid)).not.toContain(99);
  });
  it('keeps metrics for a pinned root after exec without confusing status health with ownership', async () => {
    const h = harness([snapshot([shell(), proc(11, 10)])]);
    await h.collect([entry()]);
    h.step([snapshot([proc(10, 1), proc(11, 10), proc(12, 10)])]);
    const [stat] = await h.collect([entry()]);
    expect(stat.processes.map(p => p.pid)).toEqual([10, 11, 12]);
    expect(stat.memoryBytes).toBe(33 * 1024);
    expect(stat.elapsedSeconds).toBe(100);
    expect(stat.reason).toContain('exact process birth is still verified');
  });
  it('does not waive incomplete descendant evidence when a pinned root execs', async () => {
    const h = harness([snapshot([shell(), proc(11, 10)])]);
    await h.collect([entry()]);
    h.step([snapshot([proc(10, 1), proc(11, 10), proc(12, 10, { accessible: false })])]);
    const [denied] = await h.collect([entry()]);
    expect(denied.memoryBytes).toBeNull(); expect(denied.reason).toContain('inaccessible');
    h.step([snapshot([proc(10, 1), proc(11, 10)], { complete: false })]);
    expect((await h.collect([entry()]))[0].memoryBytes).toBeNull();
  });
  it('does not adopt an already-execed root on its first request', async () => {
    const h = harness([snapshot([proc(10, 1, { marker: 'marker-a' }), proc(11, 10)])]);
    const [stat] = await h.collect([entry()]);
    expect(stat.processes).toEqual([]); expect(stat.memoryBytes).toBeNull(); expect(stat.elapsedSeconds).toBeNull();
  });
  it('does not treat another interactive root as a descendant', async () => {
    const h = harness([snapshot([shell(), { ...shell(20, 'b'), parentPid: 10 }, proc(21, 20)])]);
    const stats = await h.collect([entry(), entry('b', 20)]);
    expect(stats.map(s => s.processes.map(p => p.pid))).toEqual([[10], [20, 21]]);
  });
  it('rejects root PID reuse and never substitutes a matching shell', async () => {
    const h = harness([snapshot([shell(), proc(11, 10)])]);
    await h.collect([entry()]);
    h.step([snapshot([{ ...shell(), birth: 'boot:100', birthOrder: '100' }, proc(11, 10)])]);
    const [stat] = await h.collect([entry()]);
    expect(stat.processes.map(p => p.pid)).toEqual([11]);
    expect(stat.elapsedSeconds).toBeNull(); expect(stat.memoryBytes).toBeNull(); expect(stat.reason).toBeTruthy();
  });
  it('sums live cleanup descendants after a closed root exits without inventing root uptime', async () => {
    const h = harness([snapshot([shell(), proc(11, 10), proc(12, 11)])]);
    await h.collect([entry()]);
    h.step([snapshot([proc(11, 1), proc(12, 11)])]);
    const closing = { ...entry(), state: 'closed' as const, cleanupPending: true };
    const [stat] = await h.collect([closing]);
    expect(stat.processes.map(p => p.pid)).toEqual([11, 12]);
    expect(stat.memoryBytes).toBe(23 * 1024);
    expect(stat.elapsedSeconds).toBeNull(); expect(stat.reason).toContain('root has exited');
    h.step([snapshot([])]);
    const [empty] = await h.collect([closing]);
    // Cleanup is still pending. No measured owners is not proof of zero usage.
    expect(empty.memoryBytes).toBeNull(); expect(empty.processes).toEqual([]); expect(empty.elapsedSeconds).toBeNull();
  });
  it('keeps cleanup memory unknown if exit or descendant enumeration cannot be verified', async () => {
    const h = harness([snapshot([shell(), proc(11, 10)])]);
    await h.collect([entry()]);
    const closing = { ...entry(), state: 'closed' as const, cleanupPending: true };
    h.step([snapshot([proc(11, 1)], { complete: false })]);
    expect((await h.collect([closing]))[0].memoryBytes).toBeNull();
    h.step([snapshot([proc(11, 1, { accessible: false })])]);
    expect((await h.collect([closing]))[0].memoryBytes).toBeNull();
    const fresh = harness([snapshot([proc(11, 1)])]);
    expect((await fresh.collect([closing]))[0].memoryBytes).toBeNull();
  });
  it('omits a reused descendant, retains live reparented owners and drops exited ones', async () => {
    const h = harness([snapshot([shell(), proc(11, 10), proc(12, 10), proc(13, 10)])]);
    await h.collect([entry()]);
    h.step([snapshot([shell(), proc(11, 1), proc(12, 1, { birth: 'boot:120', birthOrder: '120' })])]);
    const [stat] = await h.collect([entry()]);
    expect(stat.processes.map(p => p.pid)).toEqual([10, 11]);
    expect(stat.memoryBytes).toBe(21 * 1024);
  });
  it('does not adopt a child through a reused numeric parent on later polls', async () => {
    const h = harness([snapshot([shell(), proc(20, 1), proc(30, 20)])]);
    await h.collect([entry()]);
    h.step([snapshot([shell(), proc(20, 10, { birth: 'boot:25', birthOrder: '25' }), proc(30, 20)])]);
    for (let i = 0; i < 2; i++) {
      const [stat] = await h.collect([entry()]);
      expect(stat.processes.map(p => p.pid)).toEqual([10, 20]);
    }
  });
  it('revalidates metric births and parent links rather than joining numeric PIDs', async () => {
    const h = harness([snapshot([shell(), proc(11, 10)])]);
    h.metrics.mockResolvedValue([{ domain: 'host/local', processes: [
      { pid: 10, parentPid: 1, birth: 'boot:100', memoryBytes: 123, elapsedSeconds: 500 },
      { pid: 11, parentPid: 2, birth: 'boot:11', memoryBytes: 123, elapsedSeconds: 500 },
    ] }]);
    const [stat] = await h.collect([entry()]);
    expect(stat.memoryBytes).toBeNull(); expect(stat.elapsedSeconds).toBeNull();
    expect(stat.processes.every(p => p.memoryBytes === null && p.elapsedSeconds === null)).toBe(true);
    expect(stat.reason).toContain('unavailable');
  });
  it('does not publish a partial sum when a descendant metric is missing', async () => {
    const h = harness([snapshot([shell(), proc(11, 10)])]);
    h.metrics.mockResolvedValue([{ domain: 'host/local', processes: [{ pid: 10, parentPid: 1, birth: 'boot:10', memoryBytes: 0, elapsedSeconds: 100 }] }]);
    const [stat] = await h.collect([entry()]);
    expect(stat.memoryBytes).toBeNull(); expect(stat.elapsedSeconds).toBe(100);
    expect(stat.processes[0].memoryBytes).toBe(0); expect(stat.reason).toBeTruthy();
  });
  it('keeps memory totals when only a descendant runtime is unavailable', async () => {
    const h = harness([snapshot([shell(), proc(11, 10)])]);
    h.metrics.mockResolvedValue([{ domain: 'host/local', processes: [
      { pid: 10, parentPid: 1, birth: 'boot:10', memoryBytes: 100, elapsedSeconds: 50 },
      { pid: 11, parentPid: 10, birth: 'boot:11', memoryBytes: 200, elapsedSeconds: null },
    ] }]);
    const [stat] = await h.collect([entry()]);
    expect(stat.memoryBytes).toBe(300); expect(stat.elapsedSeconds).toBe(50); expect(stat.reason).toContain('Runtime');
  });
  it('marks totals unavailable for incomplete enumeration or inaccessible descendants', async () => {
    const h = harness([snapshot([shell(), proc(11, 10)], { complete: false, reason: 'permission denied' })]);
    const [incomplete] = await h.collect([entry()]);
    expect(incomplete.memoryBytes).toBeNull(); expect(incomplete.elapsedSeconds).toBe(100); expect(incomplete.reason).toContain('permission denied');
    h.step([snapshot([shell(), proc(11, 10, { accessible: false })])]);
    const [denied] = await h.collect([entry()]);
    expect(denied.memoryBytes).toBeNull(); expect(denied.processes[1].memoryBytes).toBeNull();
  });
  it('does not fabricate stats for an unverified root or failed metric provider', async () => {
    const h = harness([snapshot([{ ...shell(), marker: 'wrong' }])]);
    const [missing] = await h.collect([entry()]);
    expect(missing).toMatchObject({ processes: [], memoryBytes: null, elapsedSeconds: null }); expect(missing.reason).toBeTruthy();
    h.step([snapshot([shell()])]); h.metrics.mockRejectedValue(new Error('timeout'));
    const [failed] = await h.collect([entry()]);
    expect(failed.processes[0].pid).toBe(10); expect(failed.memoryBytes).toBeNull(); expect(failed.reason).toBeTruthy();
  });
  it('rejects ambiguous root claims and duplicate namespaces', async () => {
    const h = harness([snapshot([shell()])]);
    const [a, b] = await h.collect([entry(), entry('b', 10, { marker: 'marker-a' })]);
    expect(a.processes).toEqual([]); expect(b.processes).toEqual([]);
    h.step([snapshot([shell()]), snapshot([shell()])]);
    expect((await h.collect([entry()]))[0].processes).toEqual([]);
  });
  it('isolates same guest PIDs by domain, excludes transport and status-only guests', async () => {
    const h = harness(['Ubuntu', 'Debian'].map(distro => snapshot([shell(), proc(11, 10)], {
      domain: 'guest/' + distro, environment: 'wsl', distro,
    })));
    const a = entry('a', 900, { environment: 'wsl', distro: 'Ubuntu' });
    const b = entry('b', 901, { environment: 'wsl', distro: 'Debian', marker: 'marker-a' });
    const stats = await h.collect([a, b]);
    expect(stats.map(s => s.processes.map(p => p.pid))).toEqual([[10, 11], [10, 11]]);
    expect(stats[0].processes[0].domain).toBe('guest/Ubuntu'); expect(stats[1].processes[0].domain).toBe('guest/Debian');
    h.step([snapshot([shell()], { environment: 'wsl', distro: 'Ubuntu', domain: 'guest/Ubuntu', statusOnly: true })]);
    expect((await h.collect([a]))[0].processes).toEqual([]);
  });
  it('drops tracking evidence when a terminal generation changes', async () => {
    const h = harness([snapshot([shell(), proc(11, 10)])]); await h.collect([entry()]);
    h.step([snapshot([{ ...shell(), birth: 'boot:100', birthOrder: '100' }, proc(11, 1)])]);
    const [stat] = await h.collect([entry('a', 10, { generation: 'new' })]);
    expect(stat.processes.map(p => p.pid)).toEqual([10]);
  });
  it('serializes concurrent collection without starting automatic polling', async () => {
    const h = harness([snapshot([shell()])]);
    await Promise.all([h.collect([entry()]), h.collect([entry()])]);
    expect(h.provider).toHaveBeenCalledTimes(2); expect(h.metrics).toHaveBeenCalledTimes(2);
    await new Promise(resolve => setTimeout(resolve, 20)); expect(h.provider).toHaveBeenCalledTimes(2);
  });
});

describe('platform metric fixtures', () => {
  it('parses Linux stat names without splitting the comm field and reads RSS in KiB', () => {
    const fields = ['S', '9', ...Array(17).fill('0'), '12345', '100000', '20'];
    expect(parseLinuxStat('10 (worker ) weird\nname) ' + fields.join(' '))).toEqual({ pid: 10, parentPid: 9, ticks: '12345', dead: false });
    expect(parseLinuxMemory('Name:\tworker\nVmSize:\t99999 kB\nVmRSS:\t1234 kB\n')).toBe(1234 * 1024);
    expect(parseLinuxMemory('VmSize: 99999 kB')).toBeNull();
    expect(parseLinuxMemory('VmRSS: 9007199254740991 kB')).toBeNull();
    expect(() => parseLinuxStat('10 bad stat')).toThrow();
  });
  it('computes Linux runtime from actual clock ticks, never a guessed HZ', () => {
    expect(linuxElapsed('12345', 200, 100)).toBeCloseTo(76.55);
    expect(linuxElapsed('12345', 200, 250)).toBeCloseTo(150.62);
    expect(linuxElapsed('12345', 100, 100)).toBeNull();
    expect(linuxElapsed('12345', 200, 0)).toBeNull();
    expect(linuxElapsed('9007199254740993', 1e20, 100)).toBeNull();
  });
  it('parses Windows exact FILETIME and WSL boot/tick identities without rounding', () => {
    const rows = parseMetricRows('\uFEFF' + JSON.stringify([
      { pid: 10, parentPid: 1, birth: '133987654321123456', memoryBytes: 123456, elapsedSeconds: 42.125 },
      { pid: 11, parentPid: 10, birth: 'boot:12345', memoryBytes: null, elapsedSeconds: null },
    ]));
    expect(rows[0].birth).toBe('133987654321123456'); expect(rows[0].elapsedSeconds).toBe(42.125);
    expect(rows[1].memoryBytes).toBeNull();
  });
  it('rejects malformed/duplicate metric identities and invalid numbers', () => {
    const row = { pid: 1, parentPid: 0, birth: 'b', memoryBytes: -1, elapsedSeconds: -1 };
    expect(parseMetricRows(JSON.stringify([row]))[0]).toMatchObject({ memoryBytes: null, elapsedSeconds: null });
    expect(() => parseMetricRows(JSON.stringify([row, row]))).toThrow();
    expect(() => parseMetricRows(JSON.stringify([{ ...row, birth: 123 }]))).toThrow();
    expect(() => parseMetricRows('{}')).toThrow();
  });
  it('brackets macOS ps RSS with exact microsecond births and rejects PID reuse', () => {
    const a = proc(10, 1, { birth: 'darwin:1000:123456' });
    const before = snapshot([a]);
    const after = snapshot([a]);
    const rows = parseDarwinPs(' 10 1 1234\n 99 1 100\n', before, after, 1100);
    expect(rows[0]).toMatchObject({ pid: 10, memoryBytes: 1234 * 1024 }); expect(rows[0].elapsedSeconds).toBeCloseTo(99.876544);
    expect(parseDarwinPs('10 1 1234', before, snapshot([{ ...a, birth: 'darwin:1000:123457' }]), 1100)).toEqual([]);
    expect(parseDarwinPs('10 2 1234', before, after, 1100)).toEqual([]);
    expect(parseDarwinPs('10 1 1234', before, { ...after, domain: 'other-host' }, 1100)).toEqual([]);
    expect(() => parseDarwinPs('bad', before, after, 1100)).toThrow();
  });
});
