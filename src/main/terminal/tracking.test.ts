import { afterEach, describe, expect, it, vi } from 'vitest';
import { execFile } from 'node:child_process';
import { access, open, readdir, readlink, readFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
vi.mock('node:child_process', () => ({ execFile: vi.fn() }));
vi.mock('node:fs/promises', async importOriginal => ({
  ...await importOriginal<typeof import('node:fs/promises')>(), access: vi.fn(async () => {}),
  open: vi.fn(), readdir: vi.fn(), readlink: vi.fn(), realpath: vi.fn(), stat: vi.fn(),
}));
import type { ProcessRule } from '../../shared/contracts';
import type { TabObservation } from '../../shared/native-port';
import { defaultSettings } from '../defaults';
import { ProcessTracker, createSystemSnapshotProvider, getProcessTrackingCapability, macTrackingHelperPath,
  type TrackingProcess, type TrackingRoot, type TrackingSnapshot, type SnapshotProvider } from './tracking';

const root = (overrides: Partial<TrackingRoot> = {}): TrackingRoot => ({
  tabId: 'tab-a', sessionId: 'session-a', generation: 'generation-a', pid: 10,
  environment: 'local', distro: null, marker: 'secret-a', ...overrides,
});
const proc = (pid: number, parentPid: number, executable = '/usr/bin/node', overrides: Partial<TrackingProcess> = {}): TrackingProcess => ({
  pid, parentPid, birth: 'boot:' + pid, birthOrder: String(pid), executable,
  argv: [executable, '/pkg/agent/cli.js'], accessible: true, ...overrides,
});
const shell = (pid = 10, overrides: Partial<TrackingProcess> = {}) => proc(pid, 1, '/bin/bash', { argv: ['bash'], ...overrides });
const snapshot = (processes: TrackingProcess[], overrides: Partial<TrackingSnapshot> = {}): TrackingSnapshot => ({
  domain: 'host/local', environment: 'local', distro: null, complete: true, processes, ...overrides,
});
const rule = (overrides: Partial<ProcessRule> = {}): ProcessRule => ({
  id: 'rule-a', label: 'Agent', enabled: true, executableBasenames: ['node'], executablePaths: [],
  scriptPathSuffixes: ['agent/cli.js'], ...overrides,
});
const trackers: ProcessTracker[] = [];
function mockDarwinPlatform() {
  Object.defineProperties(process, {
    platform: { value: 'darwin', configurable: true }, arch: { value: 'x64', configurable: true },
    getuid: { value: () => 501, configurable: true }, geteuid: { value: () => 501, configurable: true },
  });
}
const processDescriptors = new Map(['platform', 'arch', 'getuid', 'geteuid'].map(key => [key, Object.getOwnPropertyDescriptor(process, key)]));
afterEach(() => {
  trackers.splice(0).forEach(t => t.dispose());
  vi.useRealTimers();
  for (const [key, descriptor] of processDescriptors) {
    if (descriptor) Object.defineProperty(process, key, descriptor);
    else Reflect.deleteProperty(process, key);
  }
  vi.resetAllMocks();
});
function harness(initial: TrackingSnapshot[], watched = [root()], rules = [rule()]) {
  let current = initial;
  const provider = vi.fn<SnapshotProvider>(async () => current);
  const observations: TabObservation[][] = [];
  const tracker = new ProcessTracker(items => observations.push(items), provider);
  trackers.push(tracker);
  tracker.setWatch(watched, rules);
  return {
    tracker, provider, observations,
    last: () => observations.at(-1)!,
    async step(next: TrackingSnapshot[]) { current = next; await tracker.poll(); },
  };
}

describe('root executable continuity and launch attestation', () => {
  it('uses authenticated native shell birth when a system-shell environment is redacted', async () => {
    const birth = 'darwin:1711111111:000123';
    const h = harness([snapshot([shell(10, { executable: '/bin/zsh', birth, marker: null }), proc(11, 10)], { rootMarkerRequired: true })],
      [root({ shellExecutable: '/bin/zsh', authenticatedBirth: birth })]);
    await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ root: 'alive', health: 'healthy', agents: 1 });
    expect(h.tracker.resolveIdentity(root({ shellExecutable: '/bin/zsh', authenticatedBirth: birth }))?.birth).toBe(birth);
  });

  it('refuses a reused shell PID despite a matching marker when supervisor birth differs', async () => {
    const h = harness([snapshot([shell(10, { birth: 'darwin:1711111111:000124', marker: 'secret-a' })], { rootMarkerRequired: true })],
      [root({ authenticatedBirth: 'darwin:1711111111:000123' })]);
    await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ root: 'unavailable', health: 'unknown', agents: 0 });
    expect(h.last()[0]!.reason).toContain('authenticated supervisor identity');
    expect(h.tracker.resolveIdentity(root({ authenticatedBirth: 'darwin:1711111111:000123' }))).toBeNull();
  });

  it('does not waive the marker requirement without authenticated native birth', async () => {
    const h = harness([snapshot([shell(10, { birth: 'darwin:1711111111:000123', marker: null })], { rootMarkerRequired: true })]);
    await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ root: 'unavailable', health: 'unknown' });
    expect(h.last()[0]!.reason).toContain('marker');
  });

  it('does not report healthy waiting when a root execs Node before its first snapshot', async () => {
    const h = harness([snapshot([proc(10, 1, '/usr/bin/node', { marker: 'secret-a' })], { rootMarkerRequired: true })]);
    await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ root: 'unavailable', health: 'unknown', agents: 0 });
  });

  it('keeps an exact pinned birth but reports unknown when its shell executable changes', async () => {
    const h = harness([snapshot([shell(10, { marker: 'secret-a' })], { rootMarkerRequired: true })]);
    await h.tracker.poll(); expect(h.last()[0]).toMatchObject({ health: 'healthy' });
    await h.step([snapshot([proc(10, 1, '/usr/bin/node', { marker: 'secret-a' })], { rootMarkerRequired: true })]);
    expect(h.last()[0]).toMatchObject({ root: 'unavailable', health: 'unknown', agents: 0 });
    expect(h.last()[0]!.reason).toContain('executable');
    await h.step([snapshot([shell(10, { marker: 'secret-a' })], { rootMarkerRequired: true })]);
    expect(h.last()[0]).toMatchObject({ root: 'alive', health: 'healthy' });
  });

  it('checks the launched shell basename before first pinning, even for another valid shell', async () => {
    const h = harness([snapshot([shell(10, { executable: '/bin/zsh', marker: 'secret-a' })], { rootMarkerRequired: true })],
      [root({ shellExecutable: '/usr/bin/bash' })]);
    await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ root: 'unavailable', health: 'unknown' });
  });

  it('accepts a trusted custom shell basename without inventing a known shell family', async () => {
    const h = harness([snapshot([shell(10, { executable: '/opt/custom-shell', marker: 'secret-a' })], { rootMarkerRequired: true })],
      [root({ shellExecutable: '/opt/custom-shell' })]);
    await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ root: 'alive', health: 'healthy' });
  });

  it('does not substitute a marked guest child shell when the original root execs an agent', async () => {
    const r = root({ environment: 'wsl', distro: 'Ubuntu', pid: 9000, shellExecutable: '/bin/bash' });
    const h = harness([snapshot([
      proc(40, 1, '/usr/bin/node', { marker: 'secret-a' }), shell(41, { parentPid: 40, marker: 'secret-a' }), proc(42, 41),
    ], { environment: 'wsl', distro: 'Ubuntu', domain: 'host/wsl/Ubuntu' })], [r]);
    await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ root: 'unavailable', health: 'unknown', agents: 0 });
  });

  it('requires host shell, bounded birth order, and exact manager ancestry for Windows initial binding', async () => {
    Object.defineProperty(process, 'platform', { value: 'win32', configurable: true });
    const main = proc(20, 0, 'C:\\app\\electron.exe', { birthOrder: '100', accessible: false });
    const transport = proc(21, 20, 'C:\\Windows\\conhost.exe', { birthOrder: '200', accessible: false });
    const ps = shell(10, { parentPid: 21, executable: 'C:\\PowerShell\\pwsh.exe', birthOrder: '300' });
    const child = proc(11, 10, 'C:\\node.exe', { birthOrder: '400', argv: ['node.exe', 'C:\\pkg\\agent\\cli.js'] });
    const s = snapshot([main, transport, ps, child], { rootLaunchEvidenceRequired: true });
    const r = root({ shellExecutable: 'C:\\PowerShell\\PWSH.EXE', ancestorPid: 20, birthOrderBounds: { min: '300', max: '300' } });
    const h = harness([s], [root()], [rule({ executableBasenames: ['node.exe'] })]);
    await h.tracker.poll(); expect(h.last()[0]).toMatchObject({ root: 'unavailable', health: 'unknown', agents: 0 });
    h.tracker.setWatch([r], [rule({ executableBasenames: ['node.exe'] })]); await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ root: 'alive', health: 'healthy', agents: 1 });
    // Launch constraints authenticate the initial identity, not a permanent parent relationship.
    await h.step([snapshot([ps, child], { rootLaunchEvidenceRequired: true })]);
    expect(h.last()[0]).toMatchObject({ root: 'alive', health: 'healthy', agents: 1 });
  });

  it('rejects an unrelated or late reused Windows shell even when its basename matches', async () => {
    Object.defineProperty(process, 'platform', { value: 'win32', configurable: true });
    const r = root({ shellExecutable: 'C:\\pwsh.exe', ancestorPid: 20, birthOrderBounds: { min: '300', max: '301' } });
    const h = harness([snapshot([proc(20, 0, 'C:\\app.exe', { birthOrder: '100' }),
      shell(10, { parentPid: 20, executable: 'C:\\pwsh.exe', birthOrder: '400' })], { rootLaunchEvidenceRequired: true })], [r]);
    await h.tracker.poll(); expect(h.last()[0]).toMatchObject({ root: 'unavailable', health: 'unknown' });
    await h.step([snapshot([proc(20, 0, 'C:\\app.exe', { birthOrder: '100' }),
      shell(10, { parentPid: 1, executable: 'C:\\pwsh.exe', birthOrder: '300' })], { rootLaunchEvidenceRequired: true })]);
    expect(h.last()[0]).toMatchObject({ root: 'unavailable', health: 'unknown' });
    await h.step([snapshot([proc(20, 0, 'C:\\app.exe', { birthOrder: '100', birth: null }),
      shell(10, { parentPid: 20, executable: 'C:\\pwsh.exe', birthOrder: '300' })], { rootLaunchEvidenceRequired: true })]);
    expect(h.last()[0]).toMatchObject({ root: 'unavailable', health: 'unknown' });
  });

  it('copies launch bounds and rejects reversed or invalid launch evidence', async () => {
    const r = root({ birthOrderBounds: { min: '10', max: '10' } });
    const h = harness([snapshot([shell()])], [r]); r.birthOrderBounds!.min = '999';
    await h.tracker.poll(); expect(h.last()[0]).toMatchObject({ root: 'alive', health: 'healthy' });
    expect(() => h.tracker.setWatch([root({ ancestorPid: 10 })], [])).toThrow();
    expect(() => h.tracker.setWatch([root({ birthOrderBounds: { min: '11', max: '10' } })], [])).toThrow();
  });
});

describe('ProcessTracker ownership', () => {
  it('counts each identity once across matching rules, never the interactive root', async () => {
    const h = harness([snapshot([shell(), proc(11, 10), proc(12, 11), proc(15, 1)])], [root()], [rule(), rule({ id: 'duplicate' }), rule({ executableBasenames: ['bash'], scriptPathSuffixes: [] })]);
    await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ root: 'alive', health: 'healthy', agents: 2, reason: null });
    expect(h.last()[0]!.observedAt).toMatch(/^\d{4}-\d{2}-\d{2}T/);
  });

  it('retains established ownership when an intermediate exits or a process reparents', async () => {
    const h = harness([snapshot([shell(), proc(11, 10, '/bin/wrapper'), proc(12, 11)])]);
    await h.tracker.poll();
    expect(h.last()[0]!.agents).toBe(1);
    await h.step([snapshot([shell(), proc(12, 1)])]);
    expect(h.last()[0]).toMatchObject({ agents: 1, health: 'healthy' });
  });

  it('does not inherit descendant ownership after PID reuse', async () => {
    const h = harness([snapshot([shell(), proc(11, 10)])]);
    await h.tracker.poll();
    await h.step([snapshot([shell(), proc(11, 1, '/usr/bin/node', { birth: 'boot:999', birthOrder: '999' })])]);
    expect(h.last()[0]).toMatchObject({ agents: 0, health: 'healthy' });
  });

  it('pins the root birth until its generation changes', async () => {
    const h = harness([snapshot([shell(), proc(11, 10)])]);
    await h.tracker.poll();
    const replacement = snapshot([shell(10, { birth: 'boot:100', birthOrder: '100' }), proc(101, 10)]);
    await h.step([replacement]);
    expect(h.last()[0]).toMatchObject({ root: 'exited', health: 'unknown', agents: 0 });
    h.tracker.setWatch([root({ generation: 'generation-b' })], [rule()]);
    await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ root: 'alive', health: 'healthy', agents: 1 });
  });

  it('does not reparent an existing child through a reused parent, even after a clock reversal', async () => {
    const h = harness([snapshot([shell(), proc(20, 1, '/bin/bash', { birth: 'boot:200', birthOrder: '200' }), proc(30, 20, '/usr/bin/node', { birthOrder: '300' })])]);
    await h.tracker.poll();
    await h.step([snapshot([shell(), shell(20, { birth: 'boot:100', birthOrder: '100' }), proc(30, 20, '/usr/bin/node', { birthOrder: '300' })])]);
    h.tracker.setWatch([root(), root({ tabId: 'tab-b', pid: 20, marker: 'secret-b' })], [rule()]);
    await h.tracker.poll();
    expect(h.last().map(o => o.agents)).toEqual([0, 0]);
    await h.tracker.poll();
    expect(h.last().map(o => o.agents)).toEqual([0, 0]);
  });

  it('rejects a parent born after its child', async () => {
    const h = harness([snapshot([shell(10, { birthOrder: '900' }), proc(11, 10)])]);
    await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ agents: 0, health: 'unknown' });
  });

  it('does not infer new parent links without birth ordering', async () => {
    const h = harness([snapshot([shell(), proc(11, 10, '/usr/bin/node', { birthOrder: undefined })])]);
    await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ agents: 0, health: 'unknown' });
  });

  it('keeps separate ownership for nested watched shells', async () => {
    const h = harness([snapshot([shell(), proc(20, 10, '/bin/bash', { argv: ['bash'] }), proc(21, 20), proc(11, 10)])], [root(), root({ pid: 20, tabId: 'tab-b' })]);
    await h.tracker.poll();
    expect(h.last().map(o => o.agents)).toEqual([1, 1]);
  });

  it('re-evaluates changed rules without losing orphan ownership', async () => {
    const h = harness([snapshot([shell(), proc(11, 10)])]);
    await h.tracker.poll();
    await h.step([snapshot([shell(), proc(11, 1)])]);
    h.tracker.setWatch([root()], [rule({ enabled: false })]);
    await h.tracker.poll();
    expect(h.last()[0]!.agents).toBe(0);
    h.tracker.setWatch([root()], [rule()]);
    await h.tracker.poll();
    expect(h.last()[0]!.agents).toBe(1);
    h.tracker.setWatch([], [rule()]);
    await h.tracker.poll();
    h.tracker.setWatch([root()], [rule()]);
    await h.tracker.poll();
    expect(h.last()[0]!.agents).toBe(0);
  });

  it('gives executable paths precedence over basenames', async () => {
    const h = harness([snapshot([shell(), proc(11, 10), proc(12, 10, '/opt/node')])], [root()], [rule({ executablePaths: ['/opt/node'] })]);
    await h.tracker.poll();
    expect(h.last()[0]!.agents).toBe(1);
  });
});

describe('guest root discovery', () => {
  const guestRoot = root({ pid: 9000, environment: 'wsl', distro: 'Ubuntu' });
  const guest = (processes: TrackingProcess[], overrides: Partial<TrackingSnapshot> = {}) => snapshot(processes, {
    domain: 'host/wsl/Ubuntu', environment: 'wsl', distro: 'Ubuntu', ...overrides,
  });

  it('finds the earliest marked guest shell, not the host PID or marked descendant shells', async () => {
    const h = harness([
      snapshot([proc(9000, 1, 'C:\\Windows\\System32\\wsl.exe'), proc(9001, 9000)]),
      guest([shell(40, { marker: 'secret-a' }), proc(41, 40, '/bin/bash', { marker: 'secret-a' }), proc(42, 41, '/usr/bin/node', { marker: 'secret-a' }), proc(50, 1, '/usr/bin/node', { marker: 'secret-a' })]),
    ], [guestRoot]);
    await h.tracker.poll();
    // On Windows the detached marked agent contributes status, not ownership.
    expect(h.last()[0]).toMatchObject({ root: 'alive', health: 'healthy', agents: process.platform === 'win32' ? 2 : 1 });
    expect(h.tracker.resolveDescendants(guestRoot).map(p => p.pid)).toEqual([41, 42]);
    expect(h.provider.mock.calls[0]![0][0]!.pid).toBe(9000);
  });

  it('chooses one oldest shell among detached marked shells and excludes the unrelated branch', async () => {
    const h = harness([guest([shell(40, { marker: 'secret-a' }), shell(60, { marker: 'secret-a' }), proc(41, 40), proc(61, 60)])], [guestRoot]);
    await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ root: 'alive', agents: 1 });
  });

  it('does not guess when the oldest marked shells tie', async () => {
    const h = harness([guest([shell(40, { marker: 'secret-a', birthOrder: '40' }), shell(60, { marker: 'secret-a', birthOrder: '40' })])], [guestRoot]);
    await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ root: 'unavailable', health: 'unknown', agents: 0 });
  });

  it('requires a valid accessible shell and a complete discovery snapshot', async () => {
    const h = harness([guest([proc(40, 1, '/usr/bin/node', { marker: 'secret-a' })])], [guestRoot]);
    await h.tracker.poll();
    expect(h.last()[0]!.root).toBe('unavailable');
    await h.step([guest([shell(40, { marker: 'secret-a' })], { complete: false })]);
    expect(h.last()[0]!.root).toBe('unavailable');
    await h.step([guest([shell(40, { marker: 'secret-a', accessible: false }), shell(60, { marker: 'secret-a' })])]);
    expect(h.last()[0]!.root).toBe('unavailable');
  });

  it('does not rediscover a descendant as root after the original guest shell disappears', async () => {
    const h = harness([guest([shell(40, { marker: 'secret-a' }), shell(60, { parentPid: 40, marker: 'secret-a' }), proc(61, 60)])], [guestRoot]);
    await h.tracker.poll();
    await h.step([guest([shell(60, { marker: 'secret-a' }), proc(61, 60)])]);
    expect(h.last()[0]).toMatchObject({ root: 'exited', health: 'unknown', agents: 0 });
  });

  it('isolates identical PID/birth pairs across the host and guest distro domains', async () => {
    const h = harness([snapshot([shell(10), proc(11, 10)]), guest([shell(10, { marker: 'secret-a' }), proc(12, 10)]),
      snapshot([shell(10, { marker: 'secret-b' }), proc(13, 10)], { domain: 'host/wsl/Debian', environment: 'wsl', distro: 'Debian' })],
    [root(), { ...guestRoot, tabId: 'tab-ubuntu' }, root({ tabId: 'tab-debian', pid: 9001, environment: 'wsl', distro: 'Debian', marker: 'secret-b' })]);
    await h.tracker.poll();
    expect(h.last().map(o => o.agents)).toEqual([1, 1, 1]);
  });
});

describe('missing evidence', () => {
  it('reports failed inspection as unknown and retains ownership for recovery', async () => {
    const h = harness([snapshot([shell(), proc(11, 10)])]);
    await h.tracker.poll();
    h.provider.mockRejectedValueOnce(new Error('sensitive command or prompt'));
    await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ root: 'unavailable', health: 'unknown', agents: 0 });
    expect(h.last()[0]!.reason).not.toContain('sensitive');
    await h.step([snapshot([shell(), proc(11, 1)])]);
    expect(h.last()[0]!.agents).toBe(1);
  });

  it('never turns partial enumeration into healthy waiting', async () => {
    const h = harness([snapshot([shell()], { complete: false, reason: 'Enumeration denied.' })]);
    await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ root: 'alive', health: 'unknown', agents: 0, reason: 'Enumeration denied.' });
  });

  it('keeps an unreadable root unknown instead of declaring it exited', async () => {
    const h = harness([snapshot([shell()])]);
    await h.tracker.poll();
    await h.step([snapshot([shell(10, { birth: null, accessible: false })])]);
    expect(h.last()[0]).toMatchObject({ root: 'unavailable', health: 'unknown' });
    await h.step([snapshot([])]);
    expect(h.last()[0]).toMatchObject({ root: 'exited', health: 'unknown' });
  });

  it('does not use rounded birth times or changed domains as identity evidence', async () => {
    const h = harness([snapshot([shell(10, { birth: null })], { complete: false, reason: 'ps birth resolution is insufficient.' })]);
    await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ root: 'unavailable', health: 'unknown' });
    await h.step([snapshot([shell()])]);
    await h.step([snapshot([shell()], { domain: 'different-host/local' })]);
    expect(h.last()[0]).toMatchObject({ root: 'unavailable', health: 'unknown' });
  });

  it('reports inaccessible descendants without dropping known agent counts', async () => {
    const h = harness([snapshot([shell(), proc(11, 10), proc(12, 10, '/usr/bin/node', { accessible: false, birth: null })])]);
    await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ root: 'alive', health: 'unknown', agents: 1 });
  });

  it('requires argv only for relevant script rules', async () => {
    const h = harness([snapshot([shell(), proc(11, 10, '/usr/bin/node', { argv: null })])]);
    await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ health: 'unknown', agents: 0 });
    h.tracker.setWatch([root()], [rule({ scriptPathSuffixes: [] })]);
    await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ health: 'healthy', agents: 1 });
    await h.step([snapshot([shell(), proc(11, 10, '/bin/unrelated', { argv: null })])]);
    expect(h.last()[0]).toMatchObject({ health: 'healthy', agents: 0 });
  });

  it('rejects duplicate PID rows and duplicate domain evidence', async () => {
    const h = harness([snapshot([shell(), shell()])]);
    await h.tracker.poll();
    expect(h.last()[0]!.health).toBe('unknown');
    await h.step([snapshot([shell()]), snapshot([shell()], { environment: 'wsl', distro: 'Ubuntu' })]);
    expect(h.last()[0]!.health).toBe('unknown');
  });
});

describe('interpreter script slots', () => {
  it.each([
    [['node', '/pkg/agent/cli.js', 'prompt'], 1, 'healthy'],
    [['node', '/other.js', '/pkg/agent/cli.js'], 0, 'healthy'],
    [['node', '-e', '/pkg/agent/cli.js'], 0, 'healthy'],
    [['node', '--eval=/pkg/agent/cli.js'], 0, 'healthy'],
    [['node', '-p', '/pkg/agent/cli.js'], 0, 'healthy'],
    [['node', '-r', '/pkg/agent/cli.js', '/other.js'], 0, 'healthy'],
    [['node', '--title', '/pkg/agent/cli.js', '/other.js'], 0, 'healthy'],
    [['node', '--require=preload', '--no-warnings', '--inspect=9229', '/pkg/agent/cli.js'], 1, 'healthy'],
    [['node', '--loader', 'loader', '--', '/pkg/agent/cli.js'], 1, 'healthy'],
    [['node', '--require'], 0, 'unknown'],
    [['node', '--unsupported', '/pkg/agent/cli.js'], 0, 'unknown'],
    [['node', '--unsupported=value', '/pkg/agent/cli.js'], 0, 'unknown'],
    [['node', '/pkg/not-agent/cli.js'], 0, 'healthy'],
  ] as const)('uses only the Node script slot for %j', async (argv, agents, health) => {
    const h = harness([snapshot([shell(), proc(11, 10, '/usr/bin/node', { argv: [...argv] })])]);
    await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ agents, health });
  });

  it.each([
    [['pwsh', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', '/pkg/agent/cli.js'], 1, 'healthy'],
    [['pwsh', '-Command', '/pkg/agent/cli.js'], 0, 'healthy'],
    [['pwsh', '-EncodedCommand', '/pkg/agent/cli.js'], 0, 'healthy'],
    [['pwsh', '-File'], 0, 'unknown'],
    [['pwsh', '-Mystery', '/pkg/agent/cli.js'], 0, 'unknown'],
  ] as const)('uses only the PowerShell file slot for %j', async (argv, agents, health) => {
    const h = harness([snapshot([shell(), proc(11, 10, '/usr/bin/pwsh', { argv: [...argv] })])], [root()], [rule({ executableBasenames: ['pwsh'] })]);
    await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ agents, health });
  });

  it('trusts rewritten Pi argv without installation provenance, but still honors disabled rules', async () => {
    const pi = proc(11, 10, '/usr/bin/node', { argv: ['pi', '', ''] });
    const h = harness([snapshot([shell(), pi, proc(20, 1, '/usr/bin/node', { argv: ['pi'] })])], [root()], defaultSettings.processRules);
    await h.tracker.poll(); expect(h.last()[0]).toMatchObject({ agents: 1, health: 'healthy' });
    h.tracker.setWatch([root()], defaultSettings.processRules.map(r => ({ ...r, enabled: r.label !== 'Pi' })));
    await h.tracker.poll(); expect(h.last()[0].agents).toBe(0);
  });
  it('keeps script suffix filters on legacy/custom runtime rules', async () => {
    const h = harness([snapshot([shell(), proc(11, 10, '/usr/bin/node', { argv: ['pi'] })])], [root()], [rule()]);
    await h.tracker.poll(); expect(h.last()[0].agents).toBe(0);
  });
});

describe('unified agent detection', () => {
  it.each(['win32', 'linux', 'darwin'] as const)('trusts native names, comm and rewritten argv0 for each agent on %s', async platform => {
    Object.defineProperty(process, 'platform', { value: platform, configurable: true });
    const windows = platform === 'win32';
    const runtime = windows ? 'C:\\Node Versions\\v24\\node.exe' : '/home/u/.nvm/versions/node/v24/bin/node';
    for (const builtin of defaultSettings.processRules) {
      const name = builtin.processNames![0], exe = windows ? name + '.exe' : name;
      for (const evidence of [
        { executable: windows ? 'C:\\tools\\' + exe : '/opt/bin/' + exe, argv: null },
        { executable: runtime, argv: [name, '', ''] },
        { executable: runtime, argv: null, processName: exe },
      ]) {
        const h = harness([snapshot([shell(), proc(11, 10, evidence.executable, evidence),
          proc(20, 1, evidence.executable, evidence)])], [root()], defaultSettings.processRules);
        await h.tracker.poll(); expect(h.last()[0]).toMatchObject({ agents: 1, health: 'healthy' });
        const descendants = h.tracker.resolveDescendants(root());
        h.tracker.setWatch([root()], defaultSettings.processRules.map(r => ({ ...r, enabled: r.id !== builtin.id })));
        await h.tracker.poll(); expect(h.last()[0].agents).toBe(0);
        expect(h.tracker.resolveDescendants(root())).toEqual(descendants);
      }
    }
  });
  it.each(['win32', 'linux', 'darwin'] as const)('matches custom names but never bypasses exact paths on %s', async platform => {
    Object.defineProperty(process, 'platform', { value: platform, configurable: true });
    const windows = platform === 'win32', name = windows ? 'helper.exe' : 'helper';
    const runtime = windows ? 'C:\\tools\\node.exe' : '/usr/bin/node';
    const path = windows ? 'C:\\tools\\helper.exe' : '/opt/helper';
    const custom = rule({ executableBasenames: ['helper', 'helper.exe'], scriptPathSuffixes: [] });
    const rows = [shell(), proc(11, 10, runtime, { argv: [name] }),
      proc(12, 10, runtime, { processName: name, argv: null }), proc(13, 10, path, { argv: null })];
    const h = harness([snapshot(rows)], [root()], [custom]);
    await h.tracker.poll(); expect(h.last()[0]).toMatchObject({ agents: 3, health: 'healthy' });
    h.tracker.setWatch([root()], [{ ...custom, executablePaths: [path], processNames: [name] }]);
    await h.tracker.poll(); expect(h.last()[0]).toMatchObject({ agents: 1, health: 'healthy' });
    h.tracker.setWatch([root()], [{ ...custom, executablePaths: [path + '-other'], processNames: [name] }]);
    await h.tracker.poll(); expect(h.last()[0]).toMatchObject({ agents: 0, health: 'healthy' });
    h.tracker.setWatch([root()], [{ ...custom, enabled: false }]);
    await h.tracker.poll(); expect(h.last()[0].agents).toBe(0);
  });
  it.each(['win32', 'linux', 'darwin'] as const)('uses script fallbacks across runtime install versions on %s', async platform => {
    Object.defineProperty(process, 'platform', { value: platform, configurable: true });
    for (const builtin of defaultSettings.processRules) {
      for (const runtime of ['node', 'bun']) {
        const windows = platform === 'win32';
        const exe = windows ? 'C:\\runtimes\\v24.15.0\\' + runtime + '.exe' : '/home/u/.local/share/runtime/v22.3/bin/' + runtime;
        for (const suffix of builtin.scriptPathSuffixes) {
          const script = windows ? 'C:\\pkg\\' + suffix.replaceAll('/', '\\') : '/pkg/' + suffix;
          const h = harness([snapshot([shell(), proc(11, 10, exe, { argv: [exe, script, 'prompt'] }),
            proc(12, 10, exe, { argv: [exe, '/unrelated.js', script] })])], [root()], defaultSettings.processRules);
          await h.tracker.poll(); expect(h.last()[0]).toMatchObject({ agents: 1, health: 'healthy' });
          h.tracker.setWatch([root()], defaultSettings.processRules.map(r => ({ ...r, enabled: r.id !== builtin.id })));
          await h.tracker.poll(); expect(h.last()[0].agents).toBe(0);
        }
      }
    }
  });
  it.each([
    [['bun', '/pkg/agent/cli.js'], 1, 'healthy'],
    [['bun', 'run', '/pkg/agent/cli.js'], 1, 'healthy'],
    [['bun', 'run', 'cli.js'], 0, 'healthy'],
    [['bun', 'run', 'agent-task', '/pkg/agent/cli.js'], 0, 'healthy'],
    [['bun', 'x', '/pkg/agent/cli.js'], 0, 'healthy'],
    [['bun', '-e', '/pkg/agent/cli.js'], 0, 'healthy'],
    [['bun', '--mystery', '/pkg/agent/cli.js'], 0, 'unknown'],
  ] as const)('supports Bun direct/run paths, not arbitrary subcommands: %j', async (argv, agents, health) => {
    const h = harness([snapshot([shell(), proc(11, 10, '/usr/bin/bun', { argv: [...argv] })])], [root()], [rule({ executableBasenames: ['bun'] })]);
    await h.tracker.poll(); expect(h.last()[0]).toMatchObject({ agents, health });
  });
  it('prefers a recognized title over another script rule regardless of rule order or toggles', async () => {
    const h = harness([snapshot([shell(), proc(11, 10, '/usr/bin/node', {
      argv: ['pi', '/pkg/@anthropic-ai/claude-code/cli.js'],
    })])], [root()], [...defaultSettings.processRules].reverse());
    await h.tracker.poll(); expect(h.last()[0]).toMatchObject({ agents: 1, health: 'healthy' });
    h.tracker.setWatch([root()], defaultSettings.processRules.map(r => ({ ...r, enabled: r.label !== 'Pi' })));
    await h.tracker.poll(); expect(h.last()[0]).toMatchObject({ agents: 0, health: 'healthy' });
    h.tracker.setWatch([root()], [...defaultSettings.processRules.map(r => ({ ...r, enabled: false })),
      rule({ executableBasenames: [], executablePaths: ['/usr/bin/node'], scriptPathSuffixes: [] })]);
    await h.tracker.poll(); expect(h.last()[0]).toMatchObject({ agents: 1, health: 'healthy' });
  });
  it('does not treat prompt text, near titles, or arbitrary wrapper arguments as agent names', async () => {
    const h = harness([snapshot([shell(), ...[
      ['node', 'other.js', 'pi'], ['pi '], ['not-pi'], ['node', '-e', 'pi'],
    ].map((argv, i) => proc(11 + i, 10, '/usr/bin/node', { argv })),
    proc(21, 10, '/usr/bin/python3', { argv: ['python3', defaultSettings.processRules[0].scriptPathSuffixes[0]] })])],
    [root()], defaultSettings.processRules);
    await h.tracker.poll(); expect(h.last()[0]).toMatchObject({ agents: 0, health: 'healthy' });
  });
  it.each(['win32', 'linux'] as const)('uses platform name/path case rules on %s', async platform => {
    Object.defineProperty(process, 'platform', { value: platform, configurable: true });
    const windows = platform === 'win32', executable = windows ? 'C:\\Tools\\NODE.EXE' : '/usr/bin/node';
    const h = harness([snapshot([shell(), proc(11, 10, executable, { argv: ['PI'] })])], [root()], defaultSettings.processRules);
    await h.tracker.poll(); expect(h.last()[0].agents).toBe(windows ? 1 : 0);
    h.tracker.setWatch([root()], [rule({ executablePaths: [windows ? 'c:/tools/node.exe' : '/usr/bin/NODE'], scriptPathSuffixes: [] })]);
    await h.tracker.poll(); expect(h.last()[0].agents).toBe(windows ? 1 : 0);
  });
});

describe('poll lifetime', () => {
  it('does no snapshot work at construction and polls watched processes every second', async () => {
    vi.useFakeTimers();
    const provider = vi.fn<SnapshotProvider>(async () => [snapshot([shell()])]);
    const listener = vi.fn();
    const tracker = new ProcessTracker(listener, provider);
    trackers.push(tracker);
    expect(provider).not.toHaveBeenCalled();
    tracker.setWatch([root()], []);
    await tracker.poll();
    expect(provider).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(999);
    expect(provider).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(provider).toHaveBeenCalledTimes(2);
    tracker.dispose();
    await vi.advanceTimersByTimeAsync(4000);
    await tracker.poll();
    expect(provider).toHaveBeenCalledTimes(2);
  });

  it('coalesces concurrent polls and discards a replaced watch while inspection is pending', async () => {
    let deliver!: (s: TrackingSnapshot[]) => void;
    const provider = vi.fn<SnapshotProvider>().mockImplementationOnce(() => new Promise(resolve => { deliver = resolve; }))
      .mockResolvedValue([snapshot([shell(20)])]);
    const listener = vi.fn();
    const tracker = new ProcessTracker(listener, provider);
    trackers.push(tracker);
    tracker.setWatch([root()], []);
    const pending = tracker.poll();
    expect(tracker.poll()).toBe(pending);
    tracker.setWatch([root({ tabId: 'tab-b', pid: 20 })], []);
    deliver([snapshot([shell()])]);
    await pending;
    expect(provider).toHaveBeenCalledTimes(2);
    expect(listener).toHaveBeenCalledTimes(1);
    expect(listener.mock.calls[0]![0][0]).toMatchObject({ tabId: 'tab-b', root: 'alive' });
  });

  it('registers the second tab during an in-flight poll and resolves both shim chains in one namespace', async () => {
    Object.defineProperty(process, 'platform', { value: 'win32', configurable: true });
    let deliver!: (s: TrackingSnapshot[]) => void;
    const second = root({ tabId: 'tab-b', pid: 20, marker: 'secret-b' });
    const provider = vi.fn<SnapshotProvider>().mockImplementationOnce(() => new Promise(resolve => { deliver = resolve; }))
      .mockResolvedValue([snapshot([
        shell(10, { executable: 'C:\\pwsh.exe' }), shell(20, { executable: 'C:\\pwsh.exe' }),
        proc(11, 10, 'C:\\pi.exe'), proc(12, 11, 'C:\\cmd.exe'),
        proc(21, 20, 'C:\\pi.exe'), proc(22, 21, 'C:\\cmd.exe'),
        proc(23, 22, 'C:\\node.exe', { argv: ['node.exe', 'C:\\pkg\\@earendil-works\\pi-coding-agent\\dist\\bundle\\cli.js'] }),
        proc(30, 1, 'C:\\pi.exe'),
      ])]);
    const listener = vi.fn(), tracker = new ProcessTracker(listener, provider); trackers.push(tracker);
    tracker.setWatch([root()], defaultSettings.processRules);
    const pending = tracker.poll();
    tracker.setWatch([root(), second], defaultSettings.processRules);
    expect(tracker.poll()).toBe(pending);
    deliver([snapshot([shell()])]);
    await pending;
    expect(provider).toHaveBeenCalledTimes(2);
    expect(listener).toHaveBeenCalledTimes(1);
    expect(listener.mock.calls[0]![0]).toMatchObject([{ tabId: 'tab-a', agents: 1, health: 'healthy' }, { tabId: 'tab-b', agents: 2, health: 'healthy' }]);
  });

  it('aborts disposal and never delivers late observations', async () => {
    let deliver!: (s: TrackingSnapshot[]) => void;
    const provider = vi.fn<SnapshotProvider>(() => new Promise(resolve => { deliver = resolve; }));
    const listener = vi.fn();
    const tracker = new ProcessTracker(listener, provider);
    trackers.push(tracker);
    tracker.setWatch([root()], []);
    const pending = tracker.poll();
    tracker.dispose();
    expect(provider.mock.calls[0]![1]!.aborted).toBe(true);
    deliver([snapshot([shell()])]);
    await pending;
    expect(listener).not.toHaveBeenCalled();
  });
});

describe('bounded system snapshot commands', () => {
  type Invocation = { file: string; args: string[]; options: Record<string, unknown> };
  function mockCommands(responses: Array<string | (Error & { killed?: boolean })>) {
    const calls: Invocation[] = [];
    Object.defineProperty(process, 'platform', { value: 'win32', configurable: true });
    vi.mocked(access).mockResolvedValue(undefined);
    vi.mocked(execFile).mockImplementation(((file: string, args: string[], options: Record<string, unknown>, callback: (error: Error | null, stdout: string, stderr: string) => void) => {
      // Existing collector tests have no optional running guests; dedicated tests cover that scan.
      if (args[0] === '--list') { callback(null, '', ''); return; }
      calls.push({ file, args, options });
      const response = responses.shift();
      if (response instanceof Error) callback(response, '', 'private error text');
      else callback(null, response ?? '', '');
      return {} as ReturnType<typeof execFile>;
    }) as typeof execFile);
    return calls;
  }

  it('discovers Windows PowerShell, uses fixed CIM code, and parses quoted script slots', async () => {
    const rows = [
      { ...proc(1, 0, 'C:\\app\\electron.exe'), birthOrder: '1', commandLine: 'electron.exe', argv: undefined },
      { ...shell(), executable: 'C:\\Windows\\powershell.exe', commandLine: 'powershell.exe', argv: undefined },
      { ...proc(11, 10), executable: 'C:\\Program Files\\node.exe', commandLine: '"C:\\Program Files\\node.exe" --require "C:\\pre load.js" "C:\\pkg\\agent\\cli.js" "prompt about another cli.js"', argv: undefined },
    ];
    const calls = mockCommands([JSON.stringify(rows)]);
    const listener = vi.fn();
    const tracker = new ProcessTracker(listener, createSystemSnapshotProvider());
    trackers.push(tracker);
    expect(calls).toHaveLength(0);
    tracker.setWatch([root({ shellExecutable: 'C:\\Windows\\powershell.exe', ancestorPid: 1, birthOrderBounds: { min: '10', max: '10' } })], [rule({ executableBasenames: ['NODE.EXE'] })]);
    await tracker.poll();
    expect(listener.mock.calls[0]![0][0]).toMatchObject({ root: 'alive', health: 'healthy', agents: 1 });
    expect(calls[0]!.file).toMatch(/WindowsPowerShell[\\/]v1\.0[\\/]powershell\.exe$/);
    expect(calls[0]!.args).toContain('-NonInteractive');
    expect(calls[0]!.args.at(-1)).toContain('Get-CimInstance Win32_Process');
    const code = calls[0]!.args.at(-1)!;
    expect(code).toContain('OpenProcessToken(process, 8, out token)');
    expect(code).toContain('[ShellfoxProcessOwner]::Sid([uint32]$p.ProcessId) -eq $me');
    expect(code).toContain('CloseHandle(token); CloseHandle(process)');
    expect(code).not.toContain('Invoke-CimMethod');
    expect(code).toContain('processName=$p.Name');
    expect(code.match(/Get-CimInstance Win32_Process/g)).toHaveLength(2);
    expect(code).toContain('-ne $r.birth -or [int]$p.ParentProcessId -ne $r.parentPid');
    expect(calls[0]!.args.at(-1)).not.toContain('secret-a');
    expect(calls[0]!.options).toMatchObject({ timeout: 8000, maxBuffer: 8388608, windowsHide: true });
    expect(JSON.parse((calls[0]!.options.env as NodeJS.ProcessEnv).SHELLFOX_TRACKING_SNAPSHOT!)).toEqual({ roots: [10], known: [] });
  });

  it.each([
    { executable: 'pi.exe', commandLine: 'pi.exe', expected: 1 },
    { executable: 'node.exe', commandLine: '"C:\\Node\\node.exe" "C:\\Program Files\\pnpm\\node_modules\\.pnpm\\@earendil-works+pi-coding-agent@1.0_peer_x\\node_modules\\@earendil-works\\pi-coding-agent\\dist\\bundle\\cli.js"', expected: 1 },
    { executable: 'node.exe', commandLine: 'node.exe C:/pnpm/.pnpm/pkg_peer/node_modules/@ANTHROPIC-AI/claude-code/cli.js', expected: 1 },
    { executable: 'node.exe', commandLine: 'node.exe C:/pnpm/.pnpm/@openai+codex@1_peer/node_modules/@openai/codex/bin/codex.js', expected: 1 },
    { executable: 'claude.exe', commandLine: 'claude.exe', expected: 1 },
    { executable: 'codex.exe', commandLine: 'codex.exe', expected: 1 },
    { executable: 'node.exe', commandLine: 'node.exe unrelated.js --prompt @earendil-works/pi-coding-agent/dist/bundle/cli.js', expected: 0 },
    { executable: 'node.exe', commandLine: 'node.exe "C:\\pkg\\@earendil-works\\pi-coding-agent\\dist\\\\bundle\\cli.js"', expected: 1 },
  ])('ignores CIM PID 0 and detects $executable using real Windows argv and package suffixes', async ({ executable, commandLine, expected }) => {
    const rows = [
      { pid: 0, parentPid: 0, birth: 'idle', birthOrder: '0', accessible: false, executable: null, commandLine: null },
      { ...proc(1, 0, 'C:\\app\\electron.exe'), commandLine: 'electron.exe' },
      { ...shell(), executable: 'C:\\pwsh.exe', commandLine: 'pwsh.exe' },
      { ...proc(11, 10, 'C:\\Windows\\cmd.exe'), commandLine: 'cmd.exe /c pi.cmd' },
      { ...proc(12, 11, 'C:\\Program Files\\' + executable), commandLine },
      // Matching unrelated agent must not be adopted into the embedded tab.
      { ...proc(20, 1, 'C:\\claude.exe'), commandLine: 'claude.exe' },
    ];
    mockCommands([JSON.stringify(rows)]);
    const listener = vi.fn(), tracker = new ProcessTracker(listener, createSystemSnapshotProvider()); trackers.push(tracker);
    tracker.setWatch([root({ shellExecutable: 'C:\\pwsh.exe', ancestorPid: 1, birthOrderBounds: { min: '10', max: '10' } })], defaultSettings.processRules);
    await tracker.poll();
    expect(listener.mock.calls[0]![0][0]).toMatchObject({ root: 'alive', health: 'healthy', agents: expected });
  });

  it('treats malformed or unbalanced Windows command lines as unknown for script rules', async () => {
    mockCommands([JSON.stringify([
      { ...proc(1, 0, 'C:\\app\\electron.exe'), birthOrder: '1', commandLine: 'electron.exe', argv: undefined },
      { ...shell(), commandLine: 'bash', argv: undefined },
      { ...proc(11, 10), executable: 'C:\\node.exe', commandLine: 'node.exe "C:\\pkg\\agent\\cli.js', argv: undefined },
    ])]);
    const listener = vi.fn();
    const tracker = new ProcessTracker(listener, createSystemSnapshotProvider());
    trackers.push(tracker);
    tracker.setWatch([root({ shellExecutable: '/bin/bash', ancestorPid: 1, birthOrderBounds: { min: '10', max: '10' } })], [rule({ executableBasenames: ['node.exe'] })]);
    await tracker.poll();
    expect(listener.mock.calls[0]![0][0]).toMatchObject({ agents: 0, health: 'unknown' });
  });

  it('passes distro as one execFile argument and uses actual guest rows, not host ancestry', async () => {
    const guestRows = [shell(40, { marker: 'secret-a' }), proc(41, 40)];
    const calls = mockCommands([JSON.stringify({ processes: guestRows, complete: true })]);
    const distro = 'Ubuntu; echo not-a-command';
    const watched = root({ pid: 9000, environment: 'wsl', distro });
    const provider = createSystemSnapshotProvider();
    const result = await provider([watched]);
    expect(result[0]).toMatchObject({ environment: 'wsl', distro, complete: true, processes: guestRows });
    expect(calls[0]!.args.slice(0, 5)).toEqual(['-d', distro, '--exec', 'python3', '-c']);
    expect(calls[0]!.args[5]).toContain("'/proc/' + number");
    expect(calls[0]!.args[5]).toContain('SHELLFOX_TERMINAL_MARKER=');
    expect(calls[0]!.args[5]).not.toContain(distro);
    expect(calls[0]!.args[5]).not.toContain('secret-a');
  });

  it('falls back to the fixed proc shell program when Python is missing', async () => {
    const b64 = (s: string) => Buffer.from(s).toString('base64');
    const boot = '12345678-1234-1234-1234-123456789abc';
    const row = ['R', '40', '1', '123', '1', b64('/bin/bash'), b64('bash\0\0' + '1'), b64('SHELLFOX_TERMINAL_MARKER=secret-a\0'), b64('bash')].join('\t');
    const calls = mockCommands([new Error('python3 missing'), 'SHELLFOX_TRACKING_1\t' + boot + '\n' + row + '\n']);
    const result = await createSystemSnapshotProvider()([root({ environment: 'wsl', distro: 'Ubuntu' })]);
    expect(calls).toHaveLength(2);
    expect(calls[1]!.args.slice(0, 5)).toEqual(['-d', 'Ubuntu', '--exec', 'sh', '-c']);
    expect(calls[1]!.args[5]).toContain('for d in /proc/[0-9]*');
    expect(calls[1]!.args[5]).toContain('${1##*) }');
    expect(calls[1]!.args[5]).not.toContain('set -f');
    expect(result[0]).toMatchObject({ complete: true, processes: [{ pid: 40, parentPid: 1, birth: boot + ':123', birthOrder: '123', argv: ['bash'], marker: 'secret-a', accessible: true, processName: 'bash' }] });
  });

  it('keeps failed shell field reads inaccessible and preserves empty trailing fields', async () => {
    const b64 = (s: string) => Buffer.from(s).toString('base64');
    const row = ['R', '40', '1', '123', '1', b64('/bin/bash'), b64('\0' + '0'), ''].join('\t');
    mockCommands([new Error('python missing'), 'SHELLFOX_TRACKING_1\t12345678-1234-1234-1234-123456789abc\n' + row + '\n']);
    const result = await createSystemSnapshotProvider()([root({ environment: 'wsl', distro: 'Ubuntu' })]);
    expect(result[0]!.processes[0]).toMatchObject({ argv: null, marker: null });
  });

  it('does not retry timed out or over-limit commands and does not leak stderr', async () => {
    const error = Object.assign(new Error('private command'), { killed: true });
    const calls = mockCommands([error]);
    const result = await createSystemSnapshotProvider()([root({ environment: 'wsl', distro: 'Ubuntu' })]);
    expect(calls).toHaveLength(1);
    expect(result[0]).toMatchObject({ complete: false, processes: [] });
    expect(result[0]!.reason).not.toContain('private');
  });

  it('retains verified Windows births in the bounded owner-query scope for detached descendants', async () => {
    const rows = [{ ...shell(), commandLine: 'bash' }, { ...proc(11, 10), commandLine: 'node /pkg/agent/cli.js' }];
    const calls = mockCommands([JSON.stringify(rows), JSON.stringify(rows.map(p => ({ ...p, parentPid: 1 })))]);
    const provider = createSystemSnapshotProvider();
    await provider([root()]);
    await provider([root()]);
    const scope = JSON.parse((calls[1]!.options.env as NodeJS.ProcessEnv).SHELLFOX_TRACKING_SNAPSHOT!);
    expect(scope.known).toEqual([{ pid: 10, birth: 'boot:10' }, { pid: 11, birth: 'boot:11' }]);
  });

  it('bounds the number of namespaces before launching snapshot commands', async () => {
    const calls = mockCommands([]);
    const roots = Array.from({ length: 33 }, (_, i) => root({ tabId: 'tab-' + i, environment: 'wsl', distro: 'distro-' + i }));
    const result = await createSystemSnapshotProvider()(roots);
    expect(calls).toHaveLength(0);
    expect(result).toHaveLength(33);
    expect(result.every(s => !s.complete && s.reason?.includes('limit'))).toBe(true);
  });

  it('reports missing Windows inspection executables without spawning anything', async () => {
    const calls = mockCommands([]);
    vi.mocked(access).mockRejectedValue(new Error('not installed'));
    const result = await createSystemSnapshotProvider()([root()]);
    expect(calls).toHaveLength(0);
    expect(result[0]).toMatchObject({ complete: false, processes: [] });
  });

  it('reports missing macOS helper instead of issuing unsupported sysctl CLI OIDs or ps guesses', async () => {
    const calls = mockCommands([]); mockDarwinPlatform();
    vi.mocked(access).mockRejectedValue(Object.assign(new Error('missing'), { code: 'ENOENT' }));
    const result = await createSystemSnapshotProvider()([root()]);
    expect(calls).toHaveLength(0);
    expect(result[0]).toMatchObject({ complete: false, processes: [] });
    expect(result[0]!.reason).toContain('helper is missing');
  });
});

describe('packaged macOS native helper protocol', () => {
  const helperOptions = { packaged: false, projectRoot: process.cwd(), arch: 'x64' } as const;
  const envelope = (kind: string) => ({ protocol: 1, platform: 'darwin', arch: 'x64', source: 'libproc+numeric-sysctl', ownerUid: 501, kind, reason: null });
  const capabilities = (overrides: Record<string, unknown> = {}) => ({ ...envelope('capabilities'), available: true, exactBirth: true, identityAccess: true, enumeration: true, argv: true, environmentMarker: true, termination: false, ...overrides });
  const nativeRow = (pid: number, parentPid: number, executable: string, overrides: Record<string, unknown> = {}): Record<string, unknown> => ({
    pid, parentPid, pgid: 10, sid: 10, uid: 501, realUid: 501, savedUid: 501,
    startSeconds: '1700000000', startMicroseconds: pid === 10 ? '100' : '101', identityVerified: true, accessible: true,
    executable, argv: [executable, '/pkg/agent/cli.js'], marker: null, ...overrides,
  });
  const packet = (rows: Record<string, unknown>[], overrides: Record<string, unknown> = {}) => ({ ...envelope('snapshot'), complete: true, processes: rows, ...overrides });
  const rows = () => [nativeRow(10, 1, '/usr/bin/dash', { argv: ['sh', '-l', '-i'], marker: 'secret-a' }), nativeRow(11, 10, '/opt/node')];
  function install(responses: Array<ReturnType<typeof packet> | string | Error>, capability = capabilities()) {
    mockDarwinPlatform(); vi.mocked(access).mockResolvedValue(undefined);
    const calls: Array<{ file: string; args: string[]; options: Record<string, unknown> }> = [];
    vi.mocked(execFile).mockImplementation(((file: string, args: string[], options: Record<string, unknown>, callback: (error: Error | null, stdout: string, stderr: string) => void) => {
      calls.push({ file, args, options });
      const response = args[0] === '--capabilities' ? capability : responses.shift() ?? new Error('no snapshot');
      if (response instanceof Error) callback(response, '', 'private stderr');
      else callback(null, typeof response === 'string' ? response : JSON.stringify(response), '');
      return {} as ReturnType<typeof execFile>;
    }) as typeof execFile);
    return calls;
  }

  it('source uses SDK libproc and numeric sysctl MIBs, not dotted CLI PID OIDs or hardcoded struct offsets', async () => {
    const c = await readFile(new URL('./native/darwin-process-snapshot.c', import.meta.url), 'utf8');
    expect(c).toContain('#include <libproc.h>');
    expect(c).toContain('proc_pidinfo(pid, PROC_PIDTBSDINFO');
    expect(c).toContain('proc_listallpids');
    expect(c).toContain('proc_pidpath(pid');
    expect(c).toContain('getsid(pid)');
    expect(c).toContain('{ CTL_KERN, KERN_PROC, KERN_PROC_PID, pid }');
    expect(c).toContain('{ CTL_KERN, KERN_PROCARGS2, pid }');
    expect(c).toContain('sysctl(mib, 4, &record, &size, NULL, 0)');
    expect(c).toContain('size != sizeof(record)');
    expect(c).toContain('record.kp_proc.p_pid == pid');
    expect(c).toContain('pbi_start_tvsec'); expect(c).toContain('pbi_start_tvusec');
    expect(c).toContain('stable(&a, &b)'); expect(c).toContain('stable(&a, &final)');
    expect(c).toContain('PROC_FLAG_PSUGID');
    expect(c).not.toMatch(/\b(?:kill|killpg|proc_terminate|system|popen|execv|execl)\s*\(/);
    expect(c).not.toContain('kern.proc.pid.'); expect(c).not.toContain('kern.procargs2.');
    expect(c).not.toContain('648');
    const runtime = await readFile(new URL('./tracking.ts', import.meta.url), 'utf8');
    expect(runtime).not.toContain('/usr/sbin/sysctl');
    expect(runtime).not.toContain('kern.proc.pid.'); expect(runtime).not.toContain('kern.procargs2.');
  });

  it('selects a packaged extra resource or an explicit development artifact without packaged fallback', () => {
    expect(macTrackingHelperPath({ packaged: true, resourcesPath: '/app/resources', projectRoot: '/wrong', arch: 'arm64' }))
      .toBe(join('/app/resources', 'terminal-native', 'shellfox-process-snapshot'));
    expect(macTrackingHelperPath({ packaged: false, resourcesPath: '/electron/resources', projectRoot: '/project', arch: 'arm64' }))
      .toBe(resolve('/project', 'tmp/terminal-native/darwin-arm64/shellfox-process-snapshot'));
  });

  it('uses actual ENOENT filesystem evidence to report missing packaged helper and does not spawn a fallback', async () => {
    mockDarwinPlatform();
    const actual = await vi.importActual<typeof import('node:fs/promises')>('node:fs/promises');
    vi.mocked(access).mockImplementation(actual.access);
    const options = { platform: 'darwin' as const, packaged: true, resourcesPath: resolve('tmp', 'missing-terminal-helper-' + Date.now()), arch: 'x64' };
    const capability = await getProcessTrackingCapability(options);
    expect(capability.available).toBe(false);
    expect(capability.reason).toContain('helper is missing');
    expect(capability.helperPath).toBe(macTrackingHelperPath(options));
    const s = await createSystemSnapshotProvider(options)([root()]);
    expect(s[0]).toMatchObject({ complete: false, processes: [], reason: capability.reason });
    expect(execFile).not.toHaveBeenCalled();
  });

  it('does not equate executable file presence with readiness; requires real self-preflight response', async () => {
    const calls = install([], capabilities({ available: false, reason: 'Numeric KERN_PROCARGS2 self argv preflight failed.' }));
    const cap = await getProcessTrackingCapability({ ...helperOptions, platform: 'darwin' });
    expect(cap).toMatchObject({ available: false, reason: 'Numeric KERN_PROCARGS2 self argv preflight failed.' });
    expect(calls[0]!.args).toEqual(['--capabilities']);
    expect(calls[0]!.options).toMatchObject({ timeout: 3000, maxBuffer: 16384, killSignal: 'SIGKILL' });
    expect(calls[0]!.options.env).toMatchObject({ SHELLFOX_TERMINAL_MARKER: 'shellfox-helper-preflight-v1' });
  });

  it.each([
    { protocol: 2 }, { arch: 'arm64' }, { exactBirth: false }, { identityAccess: false }, { enumeration: false },
    { environmentMarker: false }, { termination: true }, { ownerUid: 0 },
  ])('rejects incompatible or unproven capabilities %j', async overrides => {
    install([], capabilities(overrides));
    expect((await getProcessTrackingCapability({ ...helperOptions, platform: 'darwin' })).available).toBe(false);
  });

  it('reports unsupported architecture and non-executable helpers honestly', async () => {
    mockDarwinPlatform();
    expect((await getProcessTrackingCapability({ platform: 'darwin', arch: 'ia32' })).available).toBe(false);
    expect(execFile).not.toHaveBeenCalled();
    vi.mocked(access).mockRejectedValue(Object.assign(new Error('denied'), { code: 'EACCES' }));
    const cap = await getProcessTrackingCapability({ ...helperOptions, platform: 'darwin' });
    expect(cap.reason).toContain('not executable'); expect(cap.available).toBe(false);
  });

  it('counts real protocol rows using exact microseconds, credentials, SID/PGID and canonical /bin/sh image', async () => {
    const calls = install([packet(rows())]);
    const listener = vi.fn(), tracker = new ProcessTracker(listener, createSystemSnapshotProvider(helperOptions)); trackers.push(tracker);
    const r = root({ shellExecutable: '/usr/bin/dash' });
    tracker.setWatch([r], [rule()]); await tracker.poll();
    expect(listener.mock.calls[0]![0][0]).toMatchObject({ root: 'alive', agents: 1, health: 'healthy' });
    expect(tracker.resolveIdentity(r)).toMatchObject({ pid: 10, birth: 'darwin:1700000000:000100' });
    expect(tracker.resolveDescendants(r)).toEqual([expect.objectContaining({ pid: 11, birth: 'darwin:1700000000:000101' })]);
    expect(calls.map(c => c.args[0])).toEqual(['--capabilities', '--snapshot']);
    expect(calls[1]!.options).toMatchObject({ timeout: 8000, maxBuffer: 8388608, killSignal: 'SIGKILL' });
    expect(calls.every(c => c.file === macTrackingHelperPath(helperOptions))).toBe(true);
  });

  it('accepts optional libproc process names without requiring argv or changing ownership', async () => {
    install([packet([rows()[0], nativeRow(11, 10, '/opt/node', { argv: null, processName: 'pi' })])]);
    const observations: TabObservation[][] = [];
    const tracker = new ProcessTracker(items => observations.push(items), createSystemSnapshotProvider(helperOptions)); trackers.push(tracker);
    tracker.setWatch([root()], defaultSettings.processRules); await tracker.poll();
    expect(observations[0][0]).toMatchObject({ agents: 1, health: 'healthy' });
    expect(tracker.resolveDescendants(root())).toEqual([expect.objectContaining({ pid: 11, birth: 'darwin:1700000000:000101' })]);
  });
  it('does not round native birth seconds through JSON Number or Date', async () => {
    const r = rows(); r[0]!.startSeconds = '9007199254740993'; r[0]!.startMicroseconds = '1';
    install([packet(r)]);
    const tracker = new ProcessTracker(vi.fn(), createSystemSnapshotProvider(helperOptions)); trackers.push(tracker);
    tracker.setWatch([root()], []); await tracker.poll();
    expect(tracker.resolveIdentity(root())?.birth).toBe('darwin:9007199254740993:000001');
  });

  it('never adopts a reused PID whose microsecond birth changed on a later snapshot', async () => {
    const replacement = rows(); replacement[0]!.startMicroseconds = '101'; replacement[1]!.startMicroseconds = '102';
    const calls = install([packet(rows()), packet(replacement)]);
    const listener = vi.fn(), tracker = new ProcessTracker(listener, createSystemSnapshotProvider(helperOptions)); trackers.push(tracker);
    tracker.setWatch([root()], [rule()]); await tracker.poll(); await tracker.poll();
    expect(listener.mock.calls.at(-1)![0][0]).toMatchObject({ root: 'exited', agents: 0, health: 'unknown' });
    expect(calls.filter(c => c.args[0] === '--capabilities')).toHaveLength(1);
  });

  it.each([
    { startSeconds: 1700000000 }, { startSeconds: '0' }, { startMicroseconds: '1000000' },
    { identityVerified: false }, { savedUid: 0 }, { uid: 0 }, { sid: -1 }, { sid: 0 },
  ])('rejects malformed or inconsistent exact-identity rows %j', async overrides => {
    install([packet([nativeRow(10, 1, '/usr/bin/dash', { marker: 'secret-a', ...overrides })])]);
    const s = await createSystemSnapshotProvider(helperOptions)([root()]);
    expect(s[0]).toMatchObject({ complete: false, processes: [] });
    expect(s[0]!.reason).toContain('malformed');
  });

  it('keeps unverified/reused identity and redacted root marker unknown', async () => {
    const invalid = nativeRow(10, 1, '/usr/bin/dash', { identityVerified: false, accessible: false, startSeconds: null, startMicroseconds: null, executable: null, argv: null });
    install([packet([invalid]), packet([nativeRow(10, 1, '/usr/bin/dash')])]);
    const listener = vi.fn(), tracker = new ProcessTracker(listener, createSystemSnapshotProvider(helperOptions)); trackers.push(tracker);
    tracker.setWatch([root()], [rule()]); await tracker.poll();
    expect(listener.mock.calls.at(-1)![0][0]).toMatchObject({ root: 'unavailable', agents: 0, health: 'unknown' });
    expect(tracker.resolveIdentity(root())).toBeNull();
    await tracker.poll();
    expect(listener.mock.calls.at(-1)![0][0]).toMatchObject({ root: 'unavailable', health: 'unknown' });
  });

  it('never infers script arguments from prompt text or absent argv', async () => {
    const r = rows(); r[1]!.argv = ['node', '/other.js', 'prompt /pkg/agent/cli.js'];
    const absent = rows(); absent[1]!.argv = null;
    install([packet(r), packet(absent)]);
    const provider = createSystemSnapshotProvider(helperOptions);
    const h = harness(await provider([root()])); await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ agents: 0, health: 'healthy' });
    await h.step(await provider([root()]));
    expect(h.last()[0]).toMatchObject({ agents: 0, health: 'unknown' });
  });

  it('keeps helper failures bounded and does not expose stderr/argv in the public reason', async () => {
    const calls = install([Object.assign(new Error('private argv'), { killed: true })]);
    const s = await createSystemSnapshotProvider(helperOptions)([root()]);
    expect(s[0]).toMatchObject({ complete: false, processes: [] });
    expect(s[0]!.reason).toContain('bound'); expect(s[0]!.reason).not.toContain('private');
    expect(calls).toHaveLength(2);
  });

  it('accepts supported arm64 native protocol rather than relying on a Darwin version/layout guess', async () => {
    install([packet(rows(), { arch: 'arm64' })], capabilities({ arch: 'arm64' }));
    const s = await createSystemSnapshotProvider({ ...helperOptions, arch: 'arm64' })([root()]);
    expect(s[0]).toMatchObject({ complete: true });
    expect(s[0]!.processes[0]).toMatchObject({ birth: 'darwin:1700000000:000100', sid: 10, pgid: 10 });
  });
});

describe('last proven descendant identity getter', () => {
  it('excludes the interactive root and deduplicates processes regardless of rules', async () => {
    const h = harness([snapshot([shell(), proc(11, 10), proc(12, 11)])], [root()], [rule(), rule({ id: 'duplicate' })]);
    await h.tracker.poll();
    const identities = h.tracker.resolveDescendants(root());
    expect(identities).toEqual([{ domain: 'host/local', pid: 11, birth: 'boot:11' }, { domain: 'host/local', pid: 12, birth: 'boot:12' }]);
    identities[0]!.birth = 'tampered';
    expect(h.tracker.resolveDescendants(root())[0]!.birth).toBe('boot:11');
  });

  it('retains proven orphan ownership across partial/failed snapshots for pidfd revalidation', async () => {
    const h = harness([snapshot([shell(), proc(11, 10), proc(12, 11)])]); await h.tracker.poll();
    await h.step([snapshot([shell(), proc(12, 1)])]);
    expect(h.tracker.resolveDescendants(root())).toEqual([{ domain: 'host/local', pid: 12, birth: 'boot:12' }]);
    h.provider.mockRejectedValueOnce(new Error('unavailable')); await h.tracker.poll();
    expect(h.tracker.resolveDescendants(root())).toHaveLength(1);
    await h.step([snapshot([shell()], { complete: false })]);
    expect(h.tracker.resolveDescendants(root())).toHaveLength(1);
  });

  it('never transfers ownership to a recycled descendant PID', async () => {
    const h = harness([snapshot([shell(), proc(11, 10)])]); await h.tracker.poll();
    await h.step([snapshot([shell(), proc(11, 1, '/usr/bin/node', { birth: 'boot:900', birthOrder: '900' })])]);
    expect(h.tracker.resolveDescendants(root())).toEqual([]);
  });

  it('does not return descendants without an authenticated pinned root', async () => {
    const h = harness([snapshot([shell(), proc(11, 10)], { rootMarkerRequired: true })]); await h.tracker.poll();
    expect(h.tracker.resolveDescendants(root())).toEqual([]);
    expect(h.tracker.resolveDescendants(root({ tabId: 'external-legacy' }))).toEqual([]);
  });

  it('excludes other interactive roots and their descendants', async () => {
    const second = root({ pid: 20, tabId: 'tab-b' });
    const h = harness([snapshot([shell(), shell(20, { parentPid: 10 }), proc(21, 20), proc(11, 10)])], [root(), second]);
    await h.tracker.poll();
    expect(h.tracker.resolveDescendants(root())).toEqual([{ domain: 'host/local', pid: 11, birth: 'boot:11' }]);
    expect(h.tracker.resolveDescendants(second)).toEqual([{ domain: 'host/local', pid: 21, birth: 'boot:21' }]);
  });

  it('uses the actual WSL guest domain/PID/birth, not the host transport PID', async () => {
    const r = root({ pid: 9000, environment: 'wsl', distro: 'Ubuntu' });
    const h = harness([snapshot([shell(40, { marker: 'secret-a' }), proc(41, 40)], { domain: 'host/wsl/Ubuntu', environment: 'wsl', distro: 'Ubuntu' })], [r]);
    await h.tracker.poll();
    expect(h.tracker.resolveDescendants(r)).toEqual([{ domain: 'host/wsl/Ubuntu', pid: 41, birth: 'boot:41' }]);
    expect(h.tracker.resolveIdentity(r)?.pid).toBe(40);
  });

  it('removes identities on watch removal, generation replacement, and disposal', async () => {
    const h = harness([snapshot([shell(), proc(11, 10)])]); await h.tracker.poll();
    h.tracker.setWatch([root({ generation: 'new-generation' })], [rule()]); await h.tracker.poll();
    expect(h.tracker.resolveDescendants(root())).toEqual([]);
    h.tracker.setWatch([], []); await h.tracker.poll();
    expect(h.tracker.resolveDescendants(root({ generation: 'new-generation' }))).toEqual([]);
    h.tracker.dispose(); expect(h.tracker.resolveDescendants(root())).toEqual([]);
  });
});

describe('initial local root marker authentication', () => {
  it('does not pin missing or mismatched marker evidence and recovers only after a match', async () => {
    const h = harness([snapshot([shell(), proc(11, 10)], { rootMarkerRequired: true })]);
    await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ root: 'unavailable', health: 'unknown', agents: 0 });
    await h.step([snapshot([shell(10, { marker: 'wrong' }), proc(11, 10)], { rootMarkerRequired: true })]);
    expect(h.last()[0]!.root).toBe('unavailable');
    await h.step([snapshot([shell(10, { marker: 'secret-a' }), proc(11, 10)], { rootMarkerRequired: true })]);
    expect(h.last()[0]).toMatchObject({ root: 'alive', health: 'healthy', agents: 1 });
    await h.step([snapshot([shell(10, { marker: null }), proc(11, 10)], { rootMarkerRequired: true })]);
    expect(h.last()[0]).toMatchObject({ root: 'alive', health: 'healthy', agents: 1 });
  });

  it('reads only the marker from local Linux root environ inside the before/after stat check', async () => {
    Object.defineProperties(process, { platform: { value: 'linux', configurable: true },
      getuid: { value: () => 501, configurable: true }, geteuid: { value: () => 501, configurable: true } });
    const stat = (id: number, parent: number, ticks: string) => {
      const fields = Array(20).fill('0'); fields[0] = 'S'; fields[1] = String(parent); fields[19] = ticks;
      return `${id} (test) ${fields.join(' ')}`;
    };
    const files = new Map([
      ['/proc/sys/kernel/random/boot_id', '12345678-1234-1234-1234-123456789abc'],
      ['/proc/10/stat', stat(10, 1, '100')], ['/proc/11/stat', stat(11, 10, '101')],
      ['/proc/10/status', 'Uid:\t501\t501\t501\t501\n'], ['/proc/11/status', 'Uid:\t501\t501\t501\t501\n'],
      ['/proc/10/cmdline', 'bash\0'], ['/proc/11/cmdline', 'node\0/pkg/agent/cli.js\0'],
      ['/proc/10/environ', 'TOKEN=private\0SHELLFOX_TERMINAL_MARKER=secret-a\0'],
    ]);
    const reads: string[] = [];
    vi.mocked(readdir).mockResolvedValue(['10', '11'] as unknown as Awaited<ReturnType<typeof readdir>>);
    vi.mocked(readlink).mockImplementation((async path => String(path).includes('/10/') ? '/bin/bash' : '/usr/bin/node') as typeof readlink);
    vi.mocked(open).mockImplementation(async path => {
      const name = String(path); reads.push(name);
      if (!files.has(name)) throw new Error('permission denied');
      const bytes = Buffer.from(files.get(name)!); let cursor = 0;
      return { async read(buffer: Buffer, offset: number, length: number) {
        const n = Math.min(length, bytes.length - cursor); bytes.copy(buffer, offset, cursor, cursor + n); cursor += n;
        return { bytesRead: n, buffer };
      }, async stat() { return { isFile: () => true }; }, async close() {} } as unknown as Awaited<ReturnType<typeof open>>;
    });
    const result = await createSystemSnapshotProvider()([root()]);
    expect(result[0]).toMatchObject({ complete: true, rootMarkerRequired: true });
    expect(result[0]!.processes.find(p => p.pid === 10)).toMatchObject({ marker: 'secret-a' });
    expect(reads.filter(p => p.endsWith('/environ'))).toEqual(['/proc/10/environ']);
    expect(JSON.stringify(result)).not.toContain('private');
    const h = harness(result); await h.tracker.poll();
    expect(h.last()[0]).toMatchObject({ agents: 1, health: 'healthy' });
    files.set('/proc/10/environ', 'SHELLFOX_TERMINAL_MARKER=wrong\0');
    const changed = await createSystemSnapshotProvider()([root()]);
    const other = harness(changed); await other.tracker.poll();
    expect(other.last()[0]).toMatchObject({ root: 'unavailable', health: 'unknown' });
    files.delete('/proc/10/environ');
    const denied = await createSystemSnapshotProvider()([root()]);
    const inaccessible = harness(denied); await inaccessible.tracker.poll();
    expect(inaccessible.last()[0]).toMatchObject({ root: 'unavailable', health: 'unknown' });
    // A rewritten title needs no private environ or installation provenance.
    files.set('/proc/10/environ', 'SHELLFOX_TERMINAL_MARKER=secret-a\0');
    files.set('/proc/11/cmdline', 'pi\0\0');
    reads.length = 0;
    const titled = await createSystemSnapshotProvider()([root()]);
    const named = harness(titled, [root()], defaultSettings.processRules); await named.tracker.poll();
    expect(named.last()[0]).toMatchObject({ agents: 1, health: 'healthy' });
    expect(reads.filter(p => p.endsWith('/environ'))).toEqual(['/proc/10/environ']);
    files.set('/proc/11/stat', stat(11, 10, '101').replace('(test)', '(pi)'));
    files.delete('/proc/11/cmdline');
    const comm = await createSystemSnapshotProvider()([root()]);
    expect(comm[0].processes.find(p => p.pid === 11)).toMatchObject({ processName: 'pi', argv: null });
    const byComm = harness(comm, [root()], defaultSettings.processRules); await byComm.tracker.poll();
    expect(byComm.last()[0]).toMatchObject({ agents: 1, health: 'healthy' });
  });
});
