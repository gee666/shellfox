import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { execFile } from 'node:child_process';
import { access } from 'node:fs/promises';
import type { ProcessRule } from '../../shared/contracts';
import type { TabObservation } from '../../shared/native-port';
import { ProcessTracker, createSystemSnapshotProvider, type TrackingProcess, type TrackingRoot, type TrackingSnapshot } from './tracking';
import { terminalEnvironment } from './environment';
import { PtyBackend } from './backend';
import { factoryFixture, launchInput } from './test-fixtures';
vi.mock('node:child_process', () => ({ execFile: vi.fn() }));
vi.mock('node:fs/promises', async original => ({ ...await original<typeof import('node:fs/promises')>(), access: vi.fn(async () => {}) }));
const platform = Object.getOwnPropertyDescriptor(process, 'platform')!;
const trackers: ProcessTracker[] = [];
beforeEach(() => { Object.defineProperty(process, 'platform', { value: 'win32', configurable: true }); vi.mocked(access).mockResolvedValue(undefined); });
afterEach(() => { trackers.splice(0).forEach(t => t.dispose()); Object.defineProperty(process, 'platform', platform); vi.restoreAllMocks(); vi.resetAllMocks(); });
const root = (extra: Partial<TrackingRoot> = {}): TrackingRoot => ({ tabId: 'a', sessionId: 's', generation: 'g', pid: 10, environment: 'local', distro: null, marker: 'secret-a', ...extra });
const row = (pid: number, extra: Partial<TrackingProcess> = {}): TrackingProcess => ({ pid, parentPid: 1, birth: 'boot:' + pid, birthOrder: String(pid), executable: '/usr/bin/node', argv: ['node', '/pkg/agent/cli.js'], accessible: true, marker: 'secret-a', ...extra });
const host = (): TrackingSnapshot => ({ domain: 'host', environment: 'local', distro: null, complete: true, processes: [row(10, { executable: 'C:\\pwsh.exe', argv: ['pwsh'] })] });
const guest = (processes: TrackingProcess[], extra: Partial<TrackingSnapshot> = {}): TrackingSnapshot => ({ domain: 'guest', environment: 'wsl', distro: 'Debian', statusOnly: true, complete: true, processes, ...extra });
const rule: ProcessRule = { id: 'r', label: 'Agent', enabled: true, executableBasenames: ['node'], executablePaths: [], scriptPathSuffixes: ['agent/cli.js'] };
async function observe(snapshots: TrackingSnapshot[], roots = [root()], rules = [rule]) {
  let current = snapshots;
  let items: TabObservation[] = [];
  const tracker = new ProcessTracker(next => { items = next; }, async () => current); trackers.push(tracker);
  tracker.setWatch(roots, rules); await tracker.poll();
  return { tracker, items: () => items, async step(next: TrackingSnapshot[]) { current = next; await tracker.poll(); } };
}

it.each([false, true])('protects the marker and transfers it bidirectionally on Windows, direct WSL=%s', wsl => {
  const env = terminalEnvironment({ shellfox_terminal_marker: 'inherited', WSLENV: 'OLD/up:TERM/w' }, [
    { name: 'Shellfox_Terminal_Marker', value: 'override' },
    { name: 'wslenv', value: 'KEEP/ul:TERM/w:shellfox_terminal_marker/pw:SHELLFOX_TERMINAL_MARKER/lu' },
  ], { windows: true, wsl, marker: 'owned' });
  expect(Object.keys(env).filter(k => k.toLowerCase() === 'shellfox_terminal_marker')).toEqual(['SHELLFOX_TERMINAL_MARKER']);
  expect(env.SHELLFOX_TERMINAL_MARKER).toBe('owned');
  expect(env.WSLENV).toBe('KEEP/ul:TERM:COLORTERM:SHELLFOX_TERMINAL_MARKER');
});
it('does not introduce WSLENV on native Linux', () => {
  expect(terminalEnvironment({}, [{ name: 'shellfox_terminal_marker', value: 'override' }], { windows: false, wsl: false, marker: 'owned' }))
    .toEqual({ TERM: 'xterm-256color', COLORTERM: 'truecolor', SHELLFOX_TERMINAL_MARKER: 'owned' });
});
it.each(['login-shell', 'wsl:Ubuntu'])('wires the generated backend marker into WSLENV for %s', async profileId => {
  const f = factoryFixture(), backend = new PtyBackend(f.options); await backend.initialize();
  try {
    const input = { ...launchInput(), profileId, env: [{ name: 'shellfox_terminal_marker', value: 'override' }, { name: 'WSLENV', value: 'KEEP/p:SHELLFOX_TERMINAL_MARKER/w' }] };
    expect((await backend.launch(input)).ok).toBe(true);
    const env = vi.mocked(f.factory).mock.calls[0]![2].env;
    expect(env.SHELLFOX_TERMINAL_MARKER).toBe(backend.get(input.tabId)!.root.marker);
    expect(env.SHELLFOX_TERMINAL_MARKER).not.toBe('override');
    expect(env.WSLENV.split(':')).toContain('SHELLFOX_TERMINAL_MARKER');
    expect(env.WSLENV.split(':')).toContain('KEEP/p');
  } finally { await backend.dispose(); }
});
it('counts exact markers across distros without granting guest identity or descendant authority', async () => {
  const h = await observe([host(), guest([row(10), row(11, { marker: 'other' }), row(12, { marker: null })])]);
  expect(h.items()[0]).toMatchObject({ agents: 1, health: 'healthy', root: 'alive' });
  expect(h.tracker.resolveIdentity(root())).toEqual({ domain: 'host', pid: 10, birth: 'boot:10' });
  expect(h.tracker.resolveDescendants(root())).toEqual([]);
  await h.step([host(), guest([row(10, { marker: null })])]);
  expect(h.items()[0]!.agents).toBe(0);
});
it('ignores duplicate watched markers for guest status without changing either host root', async () => {
  const a = root(), b = root({ tabId: 'b', pid: 30 });
  const s = host(); s.processes.push(row(30, { executable: 'C:\\pwsh.exe' }));
  const h = await observe([s, guest([row(20), row(21, { accessible: false, executable: null })])], [a, b]);
  expect(h.items()).toMatchObject([
    { tabId: 'a', root: 'alive', health: 'healthy', agents: 0 },
    { tabId: 'b', root: 'alive', health: 'healthy', agents: 0 },
  ]);
  for (const r of [a, b]) {
    expect(h.tracker.resolveIdentity(r)).toEqual({ domain: 'host', pid: r.pid, birth: 'boot:' + r.pid });
    expect(h.tracker.resolveDescendants(r)).toEqual([]);
  }
});
it('sums status across guest distros with identical PID/birth pairs and isolates tab markers', async () => {
  const a = root(), b = root({ tabId: 'b', pid: 30, marker: 'secret-b' });
  const s = host(); s.processes.push(row(30, { executable: 'C:\\pwsh.exe', marker: b.marker }));
  const debian = guest([row(20), row(21)]);
  const ubuntu = guest([row(20), row(22, { marker: b.marker })], { domain: 'ubuntu', distro: 'Ubuntu' });
  const h = await observe([s, debian, ubuntu], [a, b], [rule, { ...rule, id: 'also-matches' }]);
  expect(h.items()).toMatchObject([{ tabId: 'a', agents: 3, health: 'healthy' }, { tabId: 'b', agents: 1, health: 'healthy' }]);
  for (const r of [a, b]) {
    expect(h.tracker.resolveIdentity(r)?.domain).toBe('host');
    expect(h.tracker.resolveDescendants(r)).toEqual([]);
  }
  await h.step([s, ubuntu]);
  expect(h.items()).toMatchObject([{ tabId: 'a', agents: 1 }, { tabId: 'b', agents: 1 }]);
});
it('reuses direct guest evidence, counts disconnected marked agents once and keeps cleanup ancestry unchanged', async () => {
  const r = root({ environment: 'wsl', distro: 'Debian', pid: 999 });
  const h = await observe([guest([row(10, { executable: '/bin/bash', argv: ['bash'] }), row(11, { parentPid: 10 }), row(20)], { statusOnly: false })], [r], [rule, { ...rule, id: 'second' }]);
  expect(h.items()[0]).toMatchObject({ agents: 2, health: 'healthy' });
  expect(h.tracker.resolveIdentity(r)).toEqual({ domain: 'guest', pid: 10, birth: 'boot:10' });
  expect(h.tracker.resolveDescendants(r)).toEqual([{ domain: 'guest', pid: 11, birth: 'boot:11' }]);
});
it('isolates failed/unmarked/malformed optional guests and never establishes a missing root', async () => {
  const h = await observe([host(), guest([], { complete: false }), guest([row(20, { marker: 'other', accessible: false })], { domain: 'other' }), guest([row(30), row(30)], { domain: 'malformed' })]);
  expect(h.items()[0]).toMatchObject({ agents: 0, health: 'healthy' });
  await h.step([guest([row(10)])]);
  expect(h.items()[0]).toMatchObject({ root: 'unavailable', agents: 0, health: 'unknown' });
  expect(h.tracker.resolveDescendants(root())).toEqual([]);
});
it('never binds a direct WSL root from optional marker evidence', async () => {
  const r = root({ environment: 'wsl', distro: 'Debian' });
  const h = await observe([guest([row(10, { executable: '/bin/bash', argv: ['bash'] }), row(20)])], [r]);
  expect(h.items()[0]).toMatchObject({ root: 'unavailable', agents: 0 });
  expect(h.tracker.resolveIdentity(r)).toBeNull();
  expect(h.tracker.resolveDescendants(r)).toEqual([]);
});
it('scopes inaccessible marked evidence to its tab, without adopting unmarked descendants', async () => {
  const second = root({ tabId: 'b', pid: 30, marker: 'secret-b' });
  const s = host(); s.processes.push(row(30, { executable: 'C:\\pwsh.exe', marker: 'secret-b' }));
  const h = await observe([s, guest([row(20, { executable: null, accessible: false }), row(21, { parentPid: 20, marker: null })])], [root(), second]);
  expect(h.items()[0]).toMatchObject({ health: 'unknown', agents: 0 });
  expect(h.items()[1]).toMatchObject({ health: 'healthy', agents: 0 });
  await h.step([s, guest([row(20)], { complete: false })]);
  expect(h.items()[0]).toMatchObject({ health: 'unknown', agents: 1 });
  expect(h.items()[1]).toMatchObject({ health: 'healthy', agents: 0 });
});
it('uses unified guest names and script fallbacks without granting descendant authority', async () => {
  const piRule = { ...rule, processNames: ['pi'], scriptPathSuffixes: ['pi-coding-agent/dist/cli.js'] };
  const h = await observe([host(), guest([row(20, { argv: ['pi'] }), row(21, { argv: ['node', '/pkg/pi-coding-agent/dist/cli.js'] }), row(22, { argv: ['node', '-e', 'pi-coding-agent/dist/cli.js'] }), row(23, { executable: '/usr/bin/node.exe' })])], [root()], [piRule]);
  expect(h.items()[0]).toMatchObject({ agents: 2, health: 'healthy' });
  h.tracker.setWatch([root()], [{ ...piRule, enabled: false }]); await h.tracker.poll();
  expect(h.items()[0]!.agents).toBe(0);
  expect(h.tracker.resolveDescendants(root())).toEqual([]);
});
it('does not apply inherited-marker association on native Linux', async () => {
  Object.defineProperty(process, 'platform', { value: 'linux', configurable: true });
  const s = host(); s.processes[0]!.executable = '/bin/bash';
  const h = await observe([s, guest([row(20)])]);
  expect(h.items()[0]).toMatchObject({ agents: 0, health: 'healthy' });
});

type Call = { args: string[]; options: { signal?: AbortSignal; timeout: number; maxBuffer: number; encoding: string } };
function commands(respond: (args: string[]) => string | Buffer | Error) {
  const calls: Call[] = [];
  vi.mocked(execFile).mockImplementation(((file: string, args: string[], options: Call['options'], callback: (error: Error | null, stdout: string | Buffer) => void) => {
    calls.push({ args, options }); const result = respond(args);
    callback(result instanceof Error ? result : null, result instanceof Error ? '' : result);
  }) as typeof execFile);
  return calls;
}
it('lists only running distros, decodes UTF-16 names and reuses direct snapshots', async () => {
  const calls = commands(args => args[0] === '--list' ? Buffer.from('\uFEFFDebian\r\n测试\r\nDebian\r\n', 'utf16le') : JSON.stringify({ processes: [], complete: true }));
  const result = await createSystemSnapshotProvider()([root({ environment: 'wsl', distro: 'Debian' })]);
  expect(calls.map(c => c.args.slice(0, 2))).toEqual([['-d', 'Debian'], ['--list', '--running'], ['-d', '测试']]);
  expect(calls[1]!.args).toEqual(['--list', '--running', '--quiet']);
  expect(result.map(s => [s.distro, !!s.statusOnly])).toEqual([['Debian', false], ['测试', true]]);
  expect(calls[2]!.options.signal).toBeDefined();
  expect(calls[2]!.options.timeout).toBe(8000);
});
it('keeps host evidence when an optional guest fails or exceeds its command bounds', async () => {
  const calls = commands(args => args[0] === '--list' ? 'Debian\n' :
    args[0] === '-d' ? Object.assign(new Error('private guest failure'), { killed: true }) :
    JSON.stringify([{ ...host().processes[0], commandLine: 'pwsh' }]));
  const result = await createSystemSnapshotProvider()([root()]);
  expect(result).toHaveLength(1); expect(result[0]!.complete).toBe(true);
  expect(calls).toHaveLength(3);
});
it.each([new Error('WSL missing'), Array.from({ length: 33 }, (_, i) => 'D' + i).join('\n')])('optional listing failure/limit preserves host evidence', async listing => {
  const calls = commands(args => args[0] === '--list' ? listing : JSON.stringify([{ ...host().processes[0], commandLine: 'pwsh' }]));
  const result = await createSystemSnapshotProvider()([root()]);
  expect(result).toHaveLength(1); expect(result[0]!.complete).toBe(true);
  expect(calls).toHaveLength(2);
});
it.each(['disposal', 'budget'])('bounds optional workers and cancels on %s without launching remaining guests', async cause => {
  const abort = new AbortController(), budget = new AbortController(); let active = 0, maxActive = 0;
  const timeout = vi.spyOn(AbortSignal, 'timeout').mockReturnValue(budget.signal);
  const calls = commands(args => args[0] === '--list' ? Array.from({ length: 8 }, (_, i) => 'D' + i).join('\n') : '[]');
  const base = vi.mocked(execFile).getMockImplementation()!;
  vi.mocked(execFile).mockImplementation(((file: string, args: string[], options: Call['options'], callback: (error: Error | null, stdout: string) => void) => {
    if (args[0] !== '-d') return (base as Function)(file, args, options, callback);
    active++; maxActive = Math.max(maxActive, active);
    options.signal!.addEventListener('abort', () => { active--; callback(Object.assign(new Error('aborted'), { code: 'ABORT_ERR' }), ''); }, { once: true });
  }) as typeof execFile);
  const pending = createSystemSnapshotProvider()([root()], abort.signal);
  await vi.waitFor(() => expect(active).toBe(4));
  expect(timeout).toHaveBeenCalledWith(3000);
  (cause === 'disposal' ? abort : budget).abort();
  const result = await pending;
  expect(maxActive).toBe(4); expect(active).toBe(0); expect(result).toHaveLength(1);
  expect(calls).toHaveLength(2);
});
