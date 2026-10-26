import { randomUUID } from 'node:crypto';
import path from 'node:path';
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { EmbeddedSessionService } from '../main/terminal/service';
import { PtyBackend } from '../main/terminal/backend';
import { factoryFixture, MemoryRepository } from '../main/terminal/test-fixtures';
import { startHostRpc } from '../host/rpc';
import { startControlServer } from '../main/control-server';
import { runShellfoxCli } from '../main/platform/shellfox-cli-entry';
import { success } from '../shared/contracts';
import type { HostRemoteService } from '../host/rpc';
import type { RemoteState } from '../shared/remote-contracts';
const cleanup: Array<() => Promise<unknown>> = [];
afterEach(async () => { vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.unstubAllEnvs(); for (const action of cleanup.splice(0).reverse()) await action(); });
async function fixture(mode: 'desktop' | 'headless' = 'headless', remote?: HostRemoteService) {
  await mkdir('tmp', { recursive: true }); const userData = await mkdtemp(path.resolve('tmp/cli-entry-'));
  cleanup.push(() => rm(userData, { recursive: true, force: true }));
  const pty = factoryFixture(), repository = new MemoryRepository();
  const service = new EmbeddedSessionService(repository, new PtyBackend(pty.options), () => ({ setWatch() {}, dispose() {} }));
  await service.initialize(); cleanup.push(() => service.dispose());
  const shutdown = vi.fn(async () => {});
  const rpc = await startHostRpc({ userData, service, shutdown, mode, remote }); cleanup.push(() => rpc.close());
  const control = await startControlServer(userData, service, { active: false, set: active => active }); cleanup.push(() => control.dispose());
  vi.stubGlobal('__SHELLFOX_VERSION__', 'test');
  return { userData, service, pty, repository, shutdown };
}
async function run(userData: string, args: string[], prefixJson = true) {
  let stdout = '', stderr = '';
  const out = vi.spyOn(process.stdout, 'write').mockImplementation(value => { stdout += String(value); return true; });
  const err = vi.spyOn(process.stderr, 'write').mockImplementation(value => { stderr += String(value); return true; });
  try {
    const code = await runShellfoxCli(['--user-data', userData, ...(prefixJson ? ['--json'] : []), ...args]);
    return { code, value: JSON.parse(stdout), stdout, stderr };
  } finally { out.mockRestore(); err.mockRestore(); }
}
describe.each(['desktop', 'headless'] as const)('%s-owned host CLI', mode => {
  it('creates and lists through canonical and alias namespaces without stealing desktop selection', async () => {
    const f = await fixture(mode), changes: unknown[] = [];
    f.service.subscribe(event => changes.push(event));
    const created = await run(f.userData, ['session', 'create', '.']);
    expect(created.code).toBe(0); expect(created.value.ok).toBe(true); expect(created.stderr).toBe('');
    const id = created.value.value.id;
    const listed = await run(f.userData, ['sessions', 'list']);
    expect(listed.value.value.map((session: { id: string }) => session.id)).toEqual([id]);
    const shortcut = await run(f.userData, ['ss', 'list']); expect(shortcut.value.value[0].id).toBe(id);
    expect(changes.every(event => !event || typeof event !== 'object' || !('selectSessionId' in event))).toBe(true);
    expect(f.pty.processes[0].kill).not.toHaveBeenCalled(); expect(f.shutdown).not.toHaveBeenCalled();
  }, 15000);
  it('requires --yes without prompting and supports validated generic methods', async () => {
    const f = await fixture(mode);
    const created = await run(f.userData, ['sessions', 'create', '.', '--title', 'CLI project']);
    const sessionId = created.value.value.id;
    const denied = await run(f.userData, ['api', 'deleteSession', '--input', JSON.stringify({ sessionId })]);
    expect(denied.code).toBe(2); expect(denied.value.error.code).toBe('VALIDATION'); expect(denied.stderr).toBe('');
    expect(f.repository.session(sessionId)).toBeDefined();
    const renamed = await run(f.userData, ['api', 'renameSession', '--input', JSON.stringify({ sessionId, title: 'new name' })]);
    expect(renamed.value.value.title).toBe('new name');
    const deleted = await run(f.userData, ['--yes', 'session', 'rm', '--id', sessionId]);
    expect(deleted.code).toBe(0); expect(deleted.value.ok).toBe(true); expect(f.repository.session(sessionId)).toBeUndefined();
  }, 15000);
});
it('uses local remote management and --host forwarding rather than local session methods', async () => {
  const state: RemoteState = { status: 'online', loginUrl: null, error: null, enabled: true, hostname: 'local', peers: [{ id: 'peer-1', hostname: 'remote', online: true, addresses: ['100.64.0.2'], connection: 'connected', approved: true, remembered: true }], approvals: [], pending: [] };
  const remote: HostRemoteService = {
    getState: vi.fn(() => success(state)), configure: vi.fn(async () => success(state)), login: vi.fn(async () => success(state)),
    connect: vi.fn(async () => success(state)), disconnect: vi.fn(async () => success(state)), approve: vi.fn(async () => success(state)),
    revoke: vi.fn(async () => success(state)), reject: vi.fn(async () => success(state)), forget: vi.fn(async () => success(state)), subscribe: () => () => {},
    request: vi.fn(async (input: Parameters<HostRemoteService['request']>[0]) => input !== null && typeof input === 'object' && 'method' in input && input.method === 'getSessionStats' ? success({ sessions: [], tabs: [] }) : success({ cwd: '/remote/home' })),
  };
  const f = await fixture('desktop', remote);
  expect((await run(f.userData, ['machines', 'list'])).value.value[0].id).toBe('peer-1');
  const result = await run(f.userData, ['--host', 'remote', 'path', 'home']);
  expect(result.value.value.cwd).toBe('/remote/home'); expect(remote.request).toHaveBeenCalledWith(expect.objectContaining({ peerId: 'peer-1', method: 'getHomeDirectory', payload: {} }));
  const statsId = randomUUID();
  await run(f.userData, ['--host', 'remote', 'session', 'processes', statsId]);
  expect(remote.request).toHaveBeenLastCalledWith(expect.objectContaining({ peerId: 'peer-1', method: 'getSessionStats', payload: { sessionId: statsId } }));
  await run(f.userData, ['approvals', 'reject', 'peer-1', '--yes']); expect(remote.reject).toHaveBeenCalledWith({ peerId: 'peer-1' });
  expect((await f.service.getSnapshot()).ok).toBe(true); expect(f.repository.sessions()).toHaveLength(0);
}, 15000);
it('targets current sessions locally, but an explicit ID always wins over the marker', async () => {
  const f = await fixture();
  const first = await run(f.userData, ['session', 'create', '.']);
  const second = await run(f.userData, ['session', 'create', '.']);
  const marker = f.service.backend.live().find(terminal => terminal.sessionId === first.value.value.id)?.root.marker;
  if (!marker) throw new Error('Missing fixture marker');
  vi.stubEnv('SHELLFOX_TERMINAL_MARKER', marker);
  expect((await run(f.userData, ['ss', 'inspect'])).value.value.id).toBe(first.value.value.id);
  expect((await run(f.userData, ['session', 'rename', 'current name'])).value.value.title).toBe('current name');
  expect((await run(f.userData, ['session', 'rename', '--', '--help'])).value.value.title).toBe('--help');
  expect((await run(f.userData, ['session', 'rename', second.value.value.id, 'explicit name', '--current'])).value.value.id).toBe(second.value.value.id);
  expect((await run(f.userData, ['session', 'stats'])).value.value.sessions[0].id).toBe(first.value.value.id);
  vi.stubEnv('SHELLFOX_TERMINAL_MARKER', undefined);
  expect((await run(f.userData, ['session', 'inspect'])).code).toBe(2);
  expect((await run(f.userData, ['--host', 'remote', 'session', 'stats'])).value.error.message).toContain('remote session ID');
}, 25000);
it.each(['start', 'add', 'new', 'create', 'ls', 'list', 'stats', 'status', 'processes', 'proc', 'ps', 'delete', 'settle', 'unsettle', 'tab'])('rejects removed top-level %s instead of launching or mutating anything', async command => {
  const result = await run(path.resolve('tmp/no-host'), [command]);
  expect(result.code).toBe(2); expect(result.value.error.code).toBe('VALIDATION');
});
it('lets explicit --user-data override the desktop launcher default without accepting duplicate globals', async () => {
  const f = await fixture(); let stdout = '';
  const output = vi.spyOn(process.stdout, 'write').mockImplementation(value => { stdout += String(value); return true; });
  try {
    const code = await runShellfoxCli(['--shellfox-default-user-data', path.join(f.userData, 'wrong-host'), '--user-data', f.userData, 'host', 'status', '--json']);
    expect(code).toBe(0); expect(JSON.parse(stdout).value.mode).toBe('headless');
  } finally { output.mockRestore(); }
}, 15000);
it('uses one canonical commands help and accepts trailing JSON for administration and SSH errors', async () => {
  vi.stubGlobal('__SHELLFOX_VERSION__', 'test');
  const data = path.resolve('tmp/no-registry-' + randomUUID());
  for (const namespace of ['commands', 'cmd']) {
    const help = await run(data, [namespace]);
    expect(help.code).toBe(0); expect(help.value.value.match(/^Commands$/gm)).toHaveLength(1);
    expect(help.value.value).not.toContain('Custom commands:'); expect(help.value.value).not.toContain('shellfox cmd');
    const list = await run(data, [namespace, '--list', '--json'], false);
    expect(list.code).toBe(0); expect(list.value.ok).toBe(true); expect(list.stderr).toBe('');
    const usage = await run(data, [namespace, '--help', '--json'], false);
    expect(usage.code).toBe(0); expect(usage.value.value).toContain('Commands');
  }
  const ssh = await run(data, ['ssh', 'profile', '--json'], false);
  expect(ssh.code).toBe(2); expect(ssh.value.error.code).toBe('VALIDATION'); expect(ssh.stderr).toBe('');
});
it('reports argument errors and rejects JSON attach before attempting a connection', async () => {
  const missing = path.resolve('tmp/does-not-exist-' + randomUUID());
  const unknown = await run(missing, ['misspelled']); expect(unknown.code).toBe(2); expect(unknown.value.error.code).toBe('VALIDATION');
  const attach = await run(missing, ['terminals', 'attach', randomUUID()]); expect(attach.code).toBe(2); expect(attach.value.error.message).toContain('--json');
  vi.stubEnv('SHELLFOX_HEADLESS', '1'); vi.stubEnv('SHELLFOX_CLI_LAUNCH', Buffer.from(JSON.stringify({ executable: 'never-launch', args: [], env: {} })).toString('base64'));
  const start = await run(missing, ['session', 'create', '.']); expect(start.code).toBe(1); expect(start.value.error.code).toBe('HOST_UNAVAILABLE');
});
