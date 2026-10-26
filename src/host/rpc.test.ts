import { randomUUID } from 'node:crypto';
import { existsSync } from 'node:fs';
import { hostResourcePaths } from './paths';
import { mkdtemp, mkdir, rm, stat } from 'node:fs/promises';
import path from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { EmbeddedSessionService } from '../main/terminal/service';
import { failure } from '../shared/contracts';
import { PtyBackend } from '../main/terminal/backend';
import { factoryFixture, MemoryRepository } from '../main/terminal/test-fixtures';
import { startHostRpc, hostEndpointPath } from './rpc';
import { connectHostClient, type HostClient } from './client';
import { readPrivateEndpoint } from '../main/control-endpoint';
import { hostEndpointSchema } from '../shared/host-schemas';
import { hostRequest } from './client-transport';
import { acquireHostOwnership, isHostRunning } from './ownership';
import { HostRuntime } from './runtime';
import { startControlServer } from '../main/control-server';

const cleanup: Array<() => Promise<unknown>> = [];
afterEach(async () => { for (const action of cleanup.splice(0).reverse()) await action(); });
async function directory() {
  await mkdir('tmp', { recursive: true }); const dir = await mkdtemp(path.resolve('tmp/host-test-'));
  cleanup.push(() => rm(dir, { recursive: true, force: true })); return dir;
}
async function fixture() {
  const userData = await directory(), pty = factoryFixture(), repository = new MemoryRepository();
  const service = new EmbeddedSessionService(repository, new PtyBackend(pty.options), () => ({ setWatch() {}, dispose() {} }));
  await service.initialize(); cleanup.push(() => service.dispose());
  const shutdown = vi.fn(async () => {});
  const rpc = await startHostRpc({ userData, service, shutdown, mode: 'headless' }); cleanup.push(() => rpc.close());
  const connect = async () => { const client = await connectHostClient(userData); cleanup.push(() => client.close()); return client; };
  return { userData, service, pty, repository, shutdown, rpc, connect };
}
async function session(client: HostClient) {
  const result = await client.service.createSession({ cwd: process.cwd(), requestId: randomUUID() });
  if (!result.ok) throw new Error(result.error.message); return result.value;
}
describe('local host RPC', () => {
  it('authenticates and validates methods and payloads before dispatch', async () => {
    const f = await fixture(); const endpoint = await readPrivateEndpoint(hostEndpointPath(f.userData), hostEndpointSchema);
    const raw = await fetch(`http://${endpoint.host}:${endpoint.port}/v1/request`, { method: 'POST', headers: { Authorization: 'Bearer ' + '0'.repeat(64) }, body: '{}' });
    expect(raw.status).toBe(403);
    const client = await f.connect(); expect((await client.status()).mode).toBe('headless');
    const target = await session(client);
    const saved = await client.service.renameSession({ sessionId: target.id, title: 'from Node' }); expect(saved.ok).toBe(true);
    const opened = await hostRequest(endpoint, undefined, 'host.open', {});
    const invalid = await fetch(`http://${endpoint.host}:${endpoint.port}/v1/request`, { method: 'POST', headers: { Authorization: 'Bearer ' + endpoint.secret }, body: JSON.stringify({ version: 1, requestId: randomUUID(), clientId: opened.clientId, method: 'deleteSession', payload: { sessionId: target.id, extra: true } }) });
    expect(invalid.status).toBe(400); expect(f.repository.session(target.id)).toBeDefined();
    expect((await client.service.chooseDirectory()).ok).toBe(false);
    const home = await client.service.getHomeDirectory(); expect(home.ok).toBe(true);
    await client.stop(); await vi.waitFor(() => expect(f.shutdown).toHaveBeenCalledOnce());
  });
  it('rejects old disable payloads before invoking integration service methods', async () => {
    const f = await fixture(), endpoint = await readPrivateEndpoint(hostEndpointPath(f.userData), hostEndpointSchema);
    const opened = await hostRequest(endpoint, undefined, 'host.open', {});
    const cli = vi.spyOn(f.service, 'setCliIntegration'), explorer = vi.spyOn(f.service, 'setExplorerIntegration');
    for (const method of ['setCliIntegration', 'setExplorerIntegration']) {
      const response = await fetch(`http://${endpoint.host}:${endpoint.port}/v1/request`, { method: 'POST', headers: { Authorization: 'Bearer ' + endpoint.secret }, body: JSON.stringify({ version: 1, requestId: randomUUID(), clientId: opened.clientId, method, payload: { installed: false } }) });
      expect(response.status).toBe(400); expect(await response.json()).toMatchObject({ ok: false });
    }
    expect(cli).not.toHaveBeenCalled(); expect(explorer).not.toHaveBeenCalled();
  });
  it('exposes validated directory creation and scoped process stats to attached host clients', async () => {
    const f = await fixture(), client = await f.connect(), cwd = path.join(f.userData, 'project', 'new folder');
    expect(await client.service.createDirectory?.({ path: '"' + cwd + '"' })).toEqual({ ok: true, value: { cwd } });
    expect((await stat(cwd)).isDirectory()).toBe(true);
    const target = await session(client);
    const stats = vi.spyOn(f.service, 'getSessionStats').mockResolvedValue({ ok: true, value: { sessions: [{ ...target, env: [] }], tabs: [] } });
    expect(await client.service.getSessionStats?.({ sessionId: target.id })).toMatchObject({ ok: true, value: { sessions: [{ id: target.id }], tabs: [] } });
    expect(stats).toHaveBeenCalledExactlyOnceWith({ sessionId: target.id });
  });
  it('isolates terminal replay and ACK credit, detach never kills, and strips global selections', async () => {
    const f = await fixture(), a = await f.connect(), b = await f.connect();
    const target = await session(a), tab = target.tabs[0];
    if (!tab.generation) throw new Error('Missing generation');
    const changes = await b.poll(); expect(changes.events.every(e => e.type !== 'changed' || !e.event.selectSessionId)).toBe(true);
    expect((await a.service.attachTerminal({ tabId: tab.id })).ok).toBe(true);
    expect((await b.service.attachTerminal({ tabId: tab.id })).ok).toBe(true);
    f.pty.processes[0].output('both views');
    const eventA = (await a.poll()).events.find(e => e.type === 'terminal' && e.event.type === 'data');
    const eventB = (await b.poll()).events.find(e => e.type === 'terminal' && e.event.type === 'data');
    expect(eventA).toEqual(eventB);
    if (eventA?.type !== 'terminal' || eventA.event.type !== 'data') throw new Error('Missing output');
    expect((await a.service.acknowledgeTerminal({ tabId: tab.id, generation: tab.generation, sequence: eventA.event.sequence })).ok).toBe(true);
    await a.service.detachTerminal({ tabId: tab.id, generation: tab.generation });
    await a.close(); expect(f.pty.processes[0].kill).not.toHaveBeenCalled();
    expect((await b.service.writeTerminal({ tabId: tab.id, generation: tab.generation, data: 'still alive' })).ok).toBe(true);
  });
  it('notifies stream subscribers when the host transport closes', async () => {
    const f = await fixture(), client = await f.connect(); const lost = vi.fn();
    client.onDisconnect(lost); client.service.subscribeTerminal(() => {});
    await f.rpc.close(); await vi.waitFor(() => expect(lost).toHaveBeenCalledOnce());
  });
});
it('does not deadlock when a committed update handler closes its own host RPC', async () => {
  const f = await fixture(); await f.rpc.close(); let finished = false;
  const rpc = await startHostRpc({ userData: f.userData, service: f.service, mode: 'desktop', shutdown: async () => {}, desktop: async method => {
    if (method === 'installUpdate') { await rpc.close(); finished = true; }
    return failure('UNSUPPORTED', 'Synthetic update completed.');
  } }); cleanup.push(() => rpc.close());
  const client = await f.connect();
  const request = client.request('installUpdate', { confirmCloseTerminals: true }).catch(() => undefined);
  await vi.waitFor(() => expect(finished).toBe(true)); await request;
});
it('flushes a waiting stop reply only after shutdown completes without self-deadlocking close', async () => {
  const f = await fixture(); await f.rpc.close(); let closed = false, replyFlushed = false;
  let finish: (() => void) | undefined; const cleanupDone = new Promise<void>(resolve => { finish = resolve; });
  const rpc = await startHostRpc({ userData: f.userData, service: f.service, mode: 'headless', shutdown: async () => { await rpc.close(); closed = true; await cleanupDone; } });
  cleanup.push(() => rpc.close()); const client = await f.connect(); const stopped = client.stop();
  await vi.waitFor(() => expect(closed).toBe(true));
  const replies = rpc.waitForStopReplies().then(() => { replyFlushed = true; });
  await Promise.resolve(); expect(replyFlushed).toBe(false); finish?.();
  await stopped; await replies; expect(replyFlushed).toBe(true);
});
it('bounds the close drain for other disconnected desktop actions', async () => {
  const f = await fixture(); await f.rpc.close();
  let release: (() => void) | undefined; let entered = false;
  const paused = new Promise<void>(resolve => { release = resolve; });
  const rpc = await startHostRpc({ userData: f.userData, service: f.service, mode: 'desktop', shutdown: async () => {}, drainTimeoutMs: 10, desktop: async () => {
    entered = true; await paused; return failure('UNSUPPORTED', 'Synthetic dialog closed.');
  } }); cleanup.push(() => rpc.close());
  const client = await f.connect(); const request = client.service.chooseDirectory().catch(() => undefined);
  await vi.waitFor(() => expect(entered).toBe(true)); await rpc.close(); release?.(); await request;
});
describe.skipIf(!existsSync(hostResourcePaths().tailscale))('host ownership and lifecycle, requires build:tailscale', () => {
  it('has exactly one winner for concurrent acquisition and releases without stale files', async () => {
    const userData = await directory(); const results = await Promise.all(Array.from({ length: 8 }, () => acquireHostOwnership(userData)));
    const owners = results.filter(lock => lock !== null); expect(owners).toHaveLength(1);
    expect(await isHostRunning(userData)).toBe(true); await owners[0].release();
    expect(await isHostRunning(userData)).toBe(false);
  });
  it('refuses a live authenticated legacy desktop before opening SQLite', async () => {
    const f = await fixture(); const control = await startControlServer(f.userData, f.service, { active: false, set: active => active }); cleanup.push(() => control.dispose());
    expect(await acquireHostOwnership(f.userData)).toBeNull();
  });
  it('waits for confirmed own-terminal cleanup before closing repository and releasing ownership', async () => {
    const f = await fixture(); await f.rpc.close();
    const calls: string[] = []; let confirm = false;
    const originalDispose = f.service.dispose.bind(f.service);
    const runtime = await HostRuntime.create({ userData: f.userData, createResources: async () => ({
      service: Object.assign(f.service, { dispose: async () => { calls.push('pty'); if (!confirm) throw new Error('unconfirmed cleanup'); await originalDispose(); } }),
      initialize: async () => {}, closeRepository: () => { calls.push('sqlite'); },
    }) });
    await expect(runtime.stop()).rejects.toThrow('unconfirmed'); expect(calls).toEqual(['pty']);
    const client = await f.connect(); expect((await client.service.getSnapshot()).ok).toBe(true);
    await expect(client.stop()).rejects.toThrow('unconfirmed');
    expect((await client.service.getSnapshot()).ok).toBe(true);
    expect(await isHostRunning(f.userData)).toBe(true); confirm = true;
    let notified = false; runtime.onStopped(() => { notified = true; });
    await client.stop(); await vi.waitFor(() => expect(notified).toBe(true));
    expect(calls).toEqual(['pty', 'pty', 'pty', 'sqlite']); expect(await isHostRunning(f.userData)).toBe(false);
  });
});
