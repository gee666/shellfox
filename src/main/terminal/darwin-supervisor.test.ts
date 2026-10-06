import { it, expect, vi } from 'vitest';
import { createServer, type Server } from 'node:net';
import { randomUUID } from 'node:crypto';
import { mkdtemp, rmdir } from 'node:fs/promises';
import path from 'node:path';
import { DarwinSupervisorControl, darwinSupervisorPath, getDarwinSupervisorCapability } from './darwin-supervisor';
import { discoverProfiles } from './profiles';
import { PtyBackend } from './backend';
import { factoryFixture, launchInput } from './test-fixtures';
const token = 'a'.repeat(64);
const status = { version: 1 as const, ok: true, state: 'open' as const, supervisorPid: 123, shellPid: 456, shellBirth: 'darwin:1711111111:000123', shellAlive: true, exitCode: null, reason: null };
async function socketFixture(reply: (request: string) => string) {
  let directory: string | undefined;
  const endpoint = process.platform === 'win32' ? `\\\\.\\pipe\\shellfox-darwin-${randomUUID()}` : path.join(directory = await mkdtemp(path.resolve('tmp/dsc-')), 's');
  const seen: string[] = [], server = createServer(socket => { let request = ''; socket.on('data', data => { request += data; if (request.includes('\n')) { seen.push(request); socket.end(reply(request)); } }); });
  await new Promise<void>((resolve, reject) => { server.once('error', reject); server.listen(endpoint, () => resolve()); });
  return { endpoint, seen, stop: async () => { await new Promise<void>(resolve => server.close(() => resolve())); if (directory) { try { await rmdir(directory); } catch { /* Node unlinks the test socket on close. */ } } } };
}
it('uses explicit packaged/development native supervisor paths without Python', () => {
  expect(darwinSupervisorPath({ packaged: true, resourcesPath: '/Applications/Shellfox.app/Contents/Resources', arch: 'arm64' })).toBe(path.join('/Applications/Shellfox.app/Contents/Resources', 'terminal-native/shellfox-terminal-supervisor'));
  expect(darwinSupervisorPath({ packaged: false, projectRoot: '/work', arch: 'x64' })).toBe(path.resolve('/work/tmp/terminal-native/darwin-x64/shellfox-terminal-supervisor'));
});
it('enables real macOS profiles only after a compatible guardian-supervisor native preflight', async () => {
  const run = vi.fn(async () => Buffer.from(JSON.stringify({ version: 1, platform: 'darwin', arch: 'x64', available: true, reason: null, ownedSession: true, guardians: true, termination: true })));
  const d = await discoverProfiles({ platform: 'darwin', env: {}, loginShell: '/bin/zsh', exists: async () => true, canonical: async p => p, supervisorOptions: { arch: 'x64', packaged: false, projectRoot: '/work' }, run });
  expect(d.defaultProfileId).toBe('login-shell'); expect(d.profiles[0]).toMatchObject({ available: true, canTerminateDescendants: true, unavailableReason: null });
  expect(run).toHaveBeenCalledWith(expect.stringContaining('shellfox-terminal-supervisor'), ['--capabilities']);
  const wrong = await getDarwinSupervisorCapability({ platform: 'darwin', arch: 'arm64', packaged: false }, run); expect(wrong.available).toBe(false);
});
it('authenticates bounded STATUS/CLOSE replies and pins the actual shell identity', async () => {
  const f = await socketFixture(request => JSON.stringify(request.endsWith('CLOSE\n') ? { ...status, state: 'closed', shellAlive: false, exitCode: 137 } : status) + '\n');
  try {
    const control = new DarwinSupervisorControl(f.endpoint, token, undefined, 1000);
    expect(await control.ready(123)).toMatchObject({ shellPid: 456, shellBirth: status.shellBirth });
    expect(await control.close()).toMatchObject({ state: 'closed', shellAlive: false });
    expect(f.seen).toEqual([`1\t${token}\tSTATUS\n`, `1\t${token}\tCLOSE\n`]);
  } finally { await f.stop(); }
});
it('refuses mismatched identity or oversized private-control responses without target signals', async () => {
  for (const reply of [JSON.stringify({ ...status, supervisorPid: 999 }) + '\n', 'x'.repeat(3000) + '\n']) {
    const f = await socketFixture(() => reply);
    try { await expect(new DarwinSupervisorControl(f.endpoint, token, undefined, 50).ready(123)).rejects.toThrow(); }
    finally { await f.stop(); }
  }
});
it('macOS backend launches the bundled supervisor and tracks its real login-shell child', async () => {
  const f = factoryFixture(), control = { ready: vi.fn(async () => status), close: vi.fn(async () => { f.processes[0].exit(137); return { ...status, state: 'closed' as const, shellAlive: false, exitCode: 137 }; }), disposeConfirmed: vi.fn(async () => {}) };
  const backend = new PtyBackend({ ...f.options, platform: 'darwin', prepareSupervisor: async (_p, cwd, marker) => ({ file: '/resources/terminal-native/shellfox-terminal-supervisor', args: ['--supervise', '/private/tmp/shellfox/s', '/bin/bash', '-l', '-i'], cwd, env: { SHELLFOX_SUPERVISOR_TOKEN: token, SHELLFOX_TERMINAL_MARKER: marker }, control }) });
  const initialized = await backend.initialize(); expect(initialized.ok).toBe(true); const input = launchInput(); expect((await backend.launch(input)).ok).toBe(true);
  expect(f.factory).toHaveBeenCalledWith('/resources/terminal-native/shellfox-terminal-supervisor', expect.arrayContaining(['--supervise', '/bin/bash']), expect.objectContaining({ env: expect.objectContaining({ SHELLFOX_SUPERVISOR_TOKEN: token }) }));
  expect(backend.get(input.tabId)?.root).toMatchObject({ pid: 456, shellExecutable: '/bin/bash', authenticatedBirth: status.shellBirth });
  f.processes[0].kill.mockImplementation(() => { throw new Error('must not signal a Darwin target PID'); });
  expect((await backend.close({ tabId: input.tabId, generation: input.generation })).ok).toBe(true); expect(control.close).toHaveBeenCalledTimes(1); expect(f.processes[0].kill).not.toHaveBeenCalled(); await backend.dispose();
});
it('does not spawn after shutdown wins an asynchronous supervisor preparation', async () => {
  const f = factoryFixture(); let resume!: () => void;
  const gate = new Promise<void>(resolve => { resume = resolve; });
  const control = { ready: vi.fn(async () => status), close: vi.fn(async () => status), disposeConfirmed: vi.fn(async () => {}) };
  const prepareSupervisor = vi.fn(async (_p: unknown, cwd: string) => { await gate; return { file: '/native/supervisor', args: [], cwd, env: {}, control }; });
  const backend = new PtyBackend({ ...f.options, platform: 'darwin', prepareSupervisor }); await backend.initialize();
  const launching = backend.launch(launchInput());
  await vi.waitFor(() => expect(prepareSupervisor).toHaveBeenCalledTimes(1));
  await backend.dispose(); resume();
  expect(await launching).toMatchObject({ ok: false, error: { code: 'NATIVE_UNAVAILABLE' } });
  expect(f.factory).not.toHaveBeenCalled(); expect(control.disposeConfirmed).toHaveBeenCalledTimes(1);
});

it('rechecks tab ownership after concurrent native preparation before spawning', async () => {
  const f = factoryFixture(); let resume!: () => void;
  const gate = new Promise<void>(resolve => { resume = resolve; });
  const control = { ready: vi.fn(async () => status), close: vi.fn(async () => { f.processes[0].exit(0); return { ...status, state: 'closed' as const, shellAlive: false }; }), disposeConfirmed: vi.fn(async () => {}) };
  const prepareSupervisor = vi.fn(async (_p: unknown, cwd: string) => { await gate; return { file: '/native/supervisor', args: [], cwd, env: {}, control }; });
  const backend = new PtyBackend({ ...f.options, platform: 'darwin', prepareSupervisor }); await backend.initialize();
  const input = launchInput(), first = backend.launch(input), second = backend.launch({ ...input, generation: randomUUID() });
  await vi.waitFor(() => expect(prepareSupervisor).toHaveBeenCalledTimes(2)); resume();
  const results = await Promise.all([first, second]);
  expect(results.filter(r => r.ok)).toHaveLength(1); expect(f.factory).toHaveBeenCalledTimes(1);
  expect(control.disposeConfirmed).toHaveBeenCalledTimes(1); await backend.dispose();
});

it('accepts trusted normal cleanup exit when natural closure races the CLOSE socket', async () => {
  const f = factoryFixture();
  const control = { ready: async () => status, close: vi.fn(async () => { f.processes[0].exit(7); throw new Error('endpoint already removed'); }), disposeConfirmed: vi.fn(async () => {}) };
  const backend = new PtyBackend({ ...f.options, platform: 'darwin', prepareSupervisor: async (_p, cwd) => ({ file: '/native/supervisor', args: [], cwd, env: {}, control }) });
  await backend.initialize(); const input = launchInput(); await backend.launch(input);
  expect(await backend.close({ tabId: input.tabId, generation: input.generation })).toMatchObject({ ok: true });
  expect(backend.get(input.tabId)).toMatchObject({ state: 'closed', exitCode: 7, cleanupPending: false });
  expect(f.processes[0].kill).not.toHaveBeenCalled(); await backend.dispose();
});

it('accepts a trusted normal supervisor exit that arrives just after CLOSE fails (reply lost)', async () => {
  const f = factoryFixture();
  const control = { ready: async () => status, close: vi.fn(async () => { setTimeout(() => f.processes[0].exit(0), 60); throw new Error('Darwin supervisor ended without a verified reply.'); }), disposeConfirmed: vi.fn(async () => {}) };
  const backend = new PtyBackend({ ...f.options, platform: 'darwin', prepareSupervisor: async (_p, cwd) => ({ file: '/native/supervisor', args: [], cwd, env: {}, control }) });
  await backend.initialize(); const input = launchInput(); await backend.launch(input);
  expect(await backend.close({ tabId: input.tabId, generation: input.generation })).toMatchObject({ ok: true });
  expect(backend.get(input.tabId)).toMatchObject({ state: 'closed', cleanupPending: false });
  expect(f.processes[0].kill).not.toHaveBeenCalled(); await backend.dispose();
});

it('still fails, with the reason, when the supervisor neither replies nor exits within the grace period', async () => {
  const f = factoryFixture();
  const control = { ready: async () => status, close: vi.fn(async () => { throw new Error('Darwin supervisor request timed out; its owned process was not killed.'); }), disposeConfirmed: vi.fn(async () => {}) };
  const backend = new PtyBackend({ ...f.options, platform: 'darwin', supervisorExitGraceMs: 100, prepareSupervisor: async (_p, cwd) => ({ file: '/native/supervisor', args: [], cwd, env: {}, control }) });
  await backend.initialize(); const input = launchInput(); await backend.launch(input);
  const started = Date.now();
  const result = await backend.close({ tabId: input.tabId, generation: input.generation });
  expect(Date.now() - started).toBeGreaterThanOrEqual(90);
  expect(result).toMatchObject({ ok: false, error: { code: 'MONITOR_UNAVAILABLE', retryable: true } });
  if (!result.ok) expect(result.error.message).toContain('request timed out');
  expect(backend.get(input.tabId)).toMatchObject({ state: 'open' });
  expect(f.processes[0].kill).not.toHaveBeenCalled();
  f.processes[0].exit(0); await backend.dispose();
});

it('does not infer cleanup from a signalled supervisor exit when CLOSE fails', async () => {
  const f = factoryFixture();
  const control = { ready: async () => status, close: vi.fn(async () => { f.processes[0].exit(0, 9); throw new Error('supervisor crashed'); }), disposeConfirmed: vi.fn(async () => {}) };
  const backend = new PtyBackend({ ...f.options, platform: 'darwin', prepareSupervisor: async (_p, cwd) => ({ file: '/native/supervisor', args: [], cwd, env: {}, control }) });
  await backend.initialize(); const input = launchInput(); await backend.launch(input);
  expect(await backend.close({ tabId: input.tabId, generation: input.generation })).toMatchObject({ ok: false, error: { code: 'MONITOR_UNAVAILABLE' } });
  expect(backend.get(input.tabId)).toMatchObject({ state: 'closed', cleanupPending: true });
  expect(await backend.launch({ ...input, generation: randomUUID() })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(f.factory).toHaveBeenCalledTimes(1);
  expect(f.processes[0].kill).not.toHaveBeenCalled();
  await expect(backend.dispose()).rejects.toThrow();
});

it('failed Darwin CLOSE retains ownership and can be retried without Unix PID fallback', async () => {
  const f = factoryFixture(); let refuse = true;
  const control = { ready: async () => status, close: vi.fn(async () => { if (refuse) throw new Error('elevated member'); f.processes[0].exit(0); return { ...status, state: 'closed' as const, shellAlive: false }; }), disposeConfirmed: async () => {} };
  const backend = new PtyBackend({ ...f.options, platform: 'darwin', supervisorExitGraceMs: 50, prepareSupervisor: async (_p, cwd) => ({ file: '/native/supervisor', args: [], cwd, env: {}, control }) }); await backend.initialize(); const input = launchInput(); await backend.launch(input);
  expect(await backend.close({ tabId: input.tabId, generation: input.generation })).toMatchObject({ ok: false, error: { code: 'MONITOR_UNAVAILABLE' } }); expect(backend.live()).toHaveLength(1); expect(f.processes[0].kill).not.toHaveBeenCalled();
  refuse = false; await backend.dispose(); expect(backend.live()).toHaveLength(0);
});
