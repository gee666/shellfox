import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';
import { randomUUID } from 'node:crypto';
import { mkdir, mkdtemp, readFile, rm, chmod, stat, writeFile, rename, unlink } from 'node:fs/promises';
import path from 'node:path';
import { createConnection, createServer, type Socket } from 'node:net';
import { startControlServer, handleControlRequest, type ControlServer } from './control-server';
import { requestControl } from './control-client';
import { controlEndpointPath, CONTROL_REQUEST_BYTES, CONTROL_TIMEOUT_MS } from './control-protocol';
import { publishControlEndpoint } from './control-endpoint';
import { EmbeddedSessionService } from './terminal/service';
import { PtyBackend } from './terminal/backend';
import { MemoryRepository, factoryFixture } from './terminal/test-fixtures';
import { collectSessionStats } from './control-stats';
import type { ControlResponse } from './control-types';
import type { Result } from '../shared/contracts';
vi.mock('./control-stats', () => ({ collectSessionStats: vi.fn(async () => []) }));

const awake = { active: false, set: vi.fn(function (this: { active: boolean }, active: boolean) { return this.active = active; }) };
let directory: string, service: EmbeddedSessionService, server: ControlServer;
let endpoint: { version: number; host: string; port: number; secret: string };
async function raw(payload: string, halfClose = false): Promise<string> {
  return new Promise((resolve, reject) => {
    const socket = createConnection({ host: endpoint.host, port: endpoint.port });
    let response = '';
    socket.setTimeout(5000, () => socket.destroy(new Error('Test connection timed out.')));
    socket.on('error', reject); socket.on('connect', () => { if (halfClose) socket.end(payload); else socket.write(payload); });
    socket.on('data', data => { response += data; }); socket.on('end', () => resolve(response));
  });
}

describe('authenticated live control transport', () => {
  beforeAll(async () => {
    const root = path.join(process.cwd(), 'tmp'); await mkdir(root, { recursive: true });
    directory = await mkdtemp(path.join(root, 'control-test-'));
    const f = factoryFixture();
    service = new EmbeddedSessionService(new MemoryRepository(), new PtyBackend(f.options), () => ({ setWatch() {}, dispose() {} }));
    await service.initialize();
    await service.createSession({ cwd: '/work', requestId: randomUUID() });
    server = await startControlServer(directory, service, awake);
    endpoint = JSON.parse(await readFile(controlEndpointPath(directory), 'utf8'));
  }, 30000);
  afterAll(async () => { await server?.dispose(); await service?.dispose(); await rm(directory, { recursive: true, force: true }); }, 30000);

  it('serves active snapshots, toggles the existing switch, and throws operation errors', async () => {
    const response = await requestControl(directory, { action: 'list' });
    expect(response.sessions).toHaveLength(1);
    expect(await requestControl(directory, { action: 'keep-alive' })).toEqual({ message: 'Keep alive enabled.' });
    expect(awake.set).toHaveBeenLastCalledWith(true);
    expect(await requestControl(directory, { action: 'keep-alive' })).toEqual({ message: 'Keep alive disabled.' });
    await expect(requestControl(directory, { action: 'delete', id: response.sessions![0]!.id })).rejects.toThrow('confirmation');
    await expect(requestControl(directory, { action: 'inspect', id: randomUUID() })).rejects.toThrow('not found');
  }, 30000);
  it('uses collectSessionStats with live roots and lists only active session details', async () => {
    const owned = service.backend.live();
    const liveTab = { tabId: owned[0]!.tabId, processes: [], memoryBytes: null, elapsedSeconds: null };
    vi.mocked(collectSessionStats).mockResolvedValueOnce([liveTab, { ...liveTab, tabId: randomUUID() }]);
    const result = await handleControlRequest(service, awake, { action: 'processes' });
    expect(collectSessionStats).toHaveBeenLastCalledWith(owned);
    expect(result.sessions).toHaveLength(1); expect(result.tabs).toEqual([liveTab]);
    await handleControlRequest(service, awake, { action: 'stats' });
    expect(collectSessionStats).toHaveBeenCalledTimes(2);
  });
  it('omits archived sessions and their live process details from all read actions', async () => {
    const created = await service.createSession({ cwd: '/archived', requestId: randomUUID() });
    if (!created.ok) throw new Error(created.error.message);
    await service.settleSession({ sessionId: created.value.id, confirmActive: true });
    const live = service.backend.live();
    expect(live).toHaveLength(2);
    vi.mocked(collectSessionStats).mockResolvedValueOnce(live.map(t => ({ tabId: t.tabId, processes: [], memoryBytes: null, elapsedSeconds: null })));
    const result = await handleControlRequest(service, awake, { action: 'stats' });
    expect(result.sessions).toHaveLength(1); expect(result.tabs).toHaveLength(1);
    expect(result.tabs![0]!.tabId).not.toBe(created.value.tabs[0]!.id);
    expect((await handleControlRequest(service, awake, { action: 'list' })).sessions).toHaveLength(1);
    await expect(handleControlRequest(service, awake, { action: 'list', id: created.value.id })).rejects.toThrow('Active session not found');
    const deleted = await service.controlSession({ action: 'delete', id: created.value.id, confirmed: true });
    expect(deleted.ok).toBe(true);
  });
  it('filters current and ID read selectors using live context, not ambient marker alone', async () => {
    const live = service.backend.live()[0]!;
    expect((await handleControlRequest(service, awake, { action: 'list', current: true, marker: live.root.marker })).sessions?.map(s => s.id)).toEqual([live.sessionId]);
    expect((await handleControlRequest(service, awake, { action: 'list', id: live.sessionId.slice(0, 8) })).sessions?.map(s => s.id)).toEqual([live.sessionId]);
    expect((await handleControlRequest(service, awake, { action: 'list', marker: randomUUID() })).sessions).toHaveLength(1);
    await expect(handleControlRequest(service, awake, { action: 'list', current: true, marker: randomUUID() })).rejects.toThrow('context');
  });
  it('inspect returns the resolved session and only its owned process details for confirmation', async () => {
    const live = service.backend.live()[0]!;
    const tab = { tabId: live.tabId, processes: [], memoryBytes: null, elapsedSeconds: null };
    vi.mocked(collectSessionStats).mockResolvedValueOnce([tab, { ...tab, tabId: randomUUID() }]);
    const result = await handleControlRequest(service, awake, { action: 'inspect', marker: live.root.marker });
    expect(result.session?.id).toBe(live.sessionId); expect(result.tabs).toEqual([tab]);
  });
  it('rejects authentication failures and strict-schema violations without invoking an action', async () => {
    const before = awake.set.mock.calls.length;
    for (const envelope of [
      { version: 1, secret: '0'.repeat(64), request: { action: 'keep-alive' } },
      { ...endpoint, request: { action: 'keep-alive' } },
      { version: 1, secret: endpoint.secret, request: { action: 'keep-alive', extra: true } },
      { version: 1, secret: endpoint.secret, request: { action: 'tab-close', id: service.backend.live()[0]!.sessionId } },
    ]) expect(JSON.parse(await raw(JSON.stringify(envelope) + '\n')).ok).toBe(false);
    expect(awake.set.mock.calls).toHaveLength(before);
  });
  it('bounds requests, rejects invalid JSON and multiple frames, and remains usable', async () => {
    for (const payload of ['{oops}\n', 'x'.repeat(CONTROL_REQUEST_BYTES + 1) + '\n', '{}\n{}\n']) expect(JSON.parse(await raw(payload)).ok).toBe(false);
    expect((await requestControl(directory, { action: 'list' })).sessions).toHaveLength(1);
  }, 15000);
  it('replies to a half-closed request stream after asynchronous service work', async () => {
    const original = service.getControlSnapshot.bind(service);
    const poll = vi.spyOn(service, 'getControlSnapshot').mockImplementationOnce(async () => {
      await new Promise(resolve => setTimeout(resolve, 30)); return original();
    });
    try {
      const response = JSON.parse(await raw(JSON.stringify({ version: 1, secret: endpoint.secret, request: { action: 'list' } }) + '\n', true));
      expect(response.ok).toBe(true); expect(response.response.sessions).toHaveLength(1);
      expect(await raw('{partial', true)).toBe('');
    } finally { poll.mockRestore(); }
  });
  it.each(['delete', 'archive', 'tab-close'] as const)('finishes %s after terminal closure disconnects its CLI socket', async action => {
    const created = await service.createSession({ cwd: '/disconnect', requestId: randomUUID() });
    if (!created.ok) throw new Error(created.error.message);
    if (action !== 'tab-close') {
      const added = await service.addTab({ sessionId: created.value.id });
      if (!added.ok) throw new Error(added.error.message);
    }
    const current = service.backend.live().find(t => t.sessionId === created.value.id)!;
    const socket = createConnection({ host: endpoint.host, port: endpoint.port });
    socket.on('error', () => {});
    const disconnected = new Promise<void>(resolve => socket.once('close', () => resolve()));
    let signal: AbortSignal | undefined, outcome: Result<ControlResponse> | undefined;
    const operation = service.controlSession.bind(service);
    const control = vi.spyOn(service, 'controlSession').mockImplementation(async (request, abort) => {
      signal = abort; const result = await operation(request, abort); outcome = result; return result;
    });
    const close = service.backend.close.bind(service.backend);
    const closing = vi.spyOn(service.backend, 'close').mockImplementation(async input => {
      const result = await close(input);
      if (input.tabId === current.tabId) {
        // Simulate the invoking shell killing its CLI child during closure.
        socket.resetAndDestroy();
        await expect.poll(() => signal?.aborted).toBe(true);
      }
      return result;
    });
    try {
      await new Promise<void>((resolve, reject) => { socket.once('connect', resolve); socket.once('error', reject); });
      socket.write(JSON.stringify({ version: 1, secret: endpoint.secret, request: {
        action, ...(action === 'delete' ? { id: created.value.id } : { marker: current.root.marker }),
        ...(action === 'tab-close' ? {} : { confirmed: true }),
      } }) + '\n');
      await disconnected;
      await expect.poll(() => outcome?.ok).toBe(true);
      expect(service.backend.live().some(t => t.sessionId === created.value.id)).toBe(false);
      if (action === 'delete') expect(service.repository.session(created.value.id)).toBeUndefined();
      else if (action === 'archive') expect(service.repository.session(created.value.id)?.settledAt).toBeTruthy();
      else expect(service.repository.tab(current.tabId)?.lifecycle).toBe('closed');
    } finally {
      closing.mockRestore(); control.mockRestore(); socket.destroy();
      if (service.repository.session(created.value.id)) await service.controlSession({ action: 'delete', id: created.value.id, confirmed: true });
    }
  });
  it('publishes private endpoint credentials and refuses unsafe permissions', async () => {
    const file = controlEndpointPath(directory);
    if (process.platform === 'win32') {
      // Replace with an inherited, unprotected file. Client must fail closed.
      const backup = file + '.backup', inherited = file + '.inherited';
      await writeFile(inherited, JSON.stringify(endpoint)); await rename(file, backup); await rename(inherited, file);
      try { await expect(requestControl(directory, { action: 'list' })).rejects.toThrow('unsafe'); }
      finally { await unlink(file); await rename(backup, file); }
      expect((await requestControl(directory, { action: 'list' })).sessions).toHaveLength(1);
    } else {
      expect((await stat(file)).mode & 0o777).toBe(0o600);
      expect((await stat(path.dirname(file))).mode & 0o777).toBe(0o700);
      await chmod(file, 0o644);
      try { await expect(requestControl(directory, { action: 'list' })).rejects.toThrow('unsafe'); }
      finally { await chmod(file, 0o600); }
    }
  }, 15000);
  it('rejects malformed, oversized or non-loopback endpoint files before connecting', async () => {
    const file = controlEndpointPath(directory), original = await readFile(file, 'utf8');
    try {
      for (const data of ['{broken', 'x'.repeat(4097), JSON.stringify({ ...endpoint, host: '192.168.1.1' }), JSON.stringify({ ...endpoint, port: 0 })]) {
        await writeFile(file, data); await expect(requestControl(directory, { action: 'list' })).rejects.toThrow('unsafe');
      }
    } finally { await writeFile(file, original); }
  }, 15000);
  it('has an absolute socket deadline for incomplete requests', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    let socket: Socket | undefined;
    try {
      socket = createConnection({ host: endpoint.host, port: endpoint.port });
      await new Promise<void>((resolve, reject) => { socket!.once('connect', resolve); socket!.once('error', reject); });
      const closed = new Promise<void>(resolve => socket!.once('close', () => resolve()));
      socket.write('{');
      await vi.advanceTimersByTimeAsync(CONTROL_TIMEOUT_MS + 1);
      await closed;
    } finally { socket?.destroy(); vi.useRealTimers(); }
  });
  it('removes only its own endpoint and makes disposal idempotent', async () => {
    await server.dispose(); await server.dispose();
    await expect(readFile(controlEndpointPath(directory))).rejects.toMatchObject({ code: 'ENOENT' });
    await expect(requestControl(directory, { action: 'list' })).rejects.toThrow('missing');
    // Recreate transport for remaining fixture cleanup.
    server = await startControlServer(directory, service, awake);
    const oldSecret = endpoint.secret;
    endpoint = JSON.parse(await readFile(controlEndpointPath(directory), 'utf8'));
    expect(endpoint.secret).not.toBe(oldSecret);
    expect(JSON.parse(await raw(JSON.stringify({ version: 1, secret: oldSecret, request: { action: 'list' } }) + '\n')).ok).toBe(false);
  }, 30000);
});

it('the client rejects invalid responses and times out unresponsive endpoints', async () => {
  const root = path.join(process.cwd(), 'tmp'); await mkdir(root, { recursive: true });
  const dir = await mkdtemp(path.join(root, 'control-client-'));
  const sockets = new Set<Socket>();
  let reply = '{"ok":true,"response":{"unknown":true}}\n';
  let connected: (() => void) | undefined;
  const listener = createServer(socket => {
    sockets.add(socket); socket.on('error', () => {}); socket.on('close', () => sockets.delete(socket));
    socket.once('data', () => { connected?.(); if (reply) socket.end(reply); });
  });
  await new Promise<void>(resolve => listener.listen(0, '127.0.0.1', resolve));
  const address = listener.address(); if (!address || typeof address === 'string') throw new Error('No test listener.');
  const remove = await publishControlEndpoint(dir, { version: 1, host: '127.0.0.1', port: address.port, secret: 'a'.repeat(64) });
  try {
    await expect(requestControl(dir, { action: 'list' })).rejects.toThrow('Invalid');
    reply = ''; vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    const connection = new Promise<void>(resolve => { connected = resolve; });
    const result = requestControl(dir, { action: 'list' });
    const rejected = expect(result).rejects.toThrow('timed out');
    await connection;
    await vi.advanceTimersByTimeAsync(CONTROL_TIMEOUT_MS + 1); await rejected;
  } finally {
    vi.useRealTimers(); for (const socket of sockets) socket.destroy();
    await new Promise<void>(resolve => listener.close(() => resolve())); await remove(); await rm(dir, { recursive: true, force: true });
  }
}, 30000);

it('does not remove credentials published by a newer server', async () => {
  const root = path.join(process.cwd(), 'tmp'); await mkdir(root, { recursive: true });
  const dir = await mkdtemp(path.join(root, 'control-replacement-'));
  const a = { version: 1, host: '127.0.0.1', port: 1234, secret: 'a'.repeat(64) };
  const remove = await publishControlEndpoint(dir, a);
  try {
    await writeFile(controlEndpointPath(dir), JSON.stringify({ ...a, secret: 'b'.repeat(64) }));
    await remove(); expect(JSON.parse(await readFile(controlEndpointPath(dir), 'utf8')).secret).toBe('b'.repeat(64));
  } finally { await rm(dir, { recursive: true, force: true }); }
}, 15000);
