import { randomUUID } from 'node:crypto';
import { afterEach, expect, it, vi } from 'vitest';
import { RemoteHost } from './host';
import { RemoteControlLeases } from './operations';
import { embeddedFixture, identity, signedHeaders } from './test-fixtures';

const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0).reverse()) await cleanup();
  vi.restoreAllMocks();
});
async function fixture() {
  const f = await embeddedFixture(), secret = 'discovery-test-secret';
  const approved = vi.fn(() => false), pending = vi.fn(), authorize = vi.fn(() => null), withdraw = vi.fn();
  const host = new RemoteHost(f.service, secret, approved, pending, authorize, withdraw);
  const url = await host.listen();
  cleanups.push(async () => { await host.dispose(); await f.service.dispose(); });
  const send = async (path: string, body = '{}', headers: Record<string, string> = signedHeaders(secret, identity, path, body)) => {
    const response = await fetch(url + path, { method: 'POST', headers, body });
    expect(response.headers.get('cache-control')).toBe('no-store');
    return response.json() as Promise<unknown>;
  };
  return { ...f, host, url, send, approved, pending, authorize, withdraw };
}
it('returns only a service signature to authenticated same-owner peers without approval or pairing', async () => {
  const f = await fixture();
  const created = await f.service.createSession({ cwd: '/work', requestId: randomUUID() });
  if (!created.ok) throw new Error('fixture session missing');
  const snapshot = vi.spyOn(f.service, 'getSnapshot'), visible = vi.spyOn(f.service, 'setRemoteVisibleSessions');
  const clear = vi.spyOn(f.service, 'clearRemoteVisibleSessions'), attach = vi.spyOn(f.service, 'attachTerminal');
  const lease = vi.spyOn(RemoteControlLeases.prototype, 'claim');
  // The body is irrelevant to discovery, including any supplied client identity.
  for (const body of ['{}', JSON.stringify({ clientId: randomUUID() }), 'null']) {
    expect(await f.send('/v1/discover', body)).toEqual({ ok: true, value: { service: 'shellfox', protocolVersion: 1 } });
  }
  expect(f.pending).not.toHaveBeenCalled(); expect(f.authorize).not.toHaveBeenCalled();
  expect(f.approved).not.toHaveBeenCalled(); expect(f.withdraw).not.toHaveBeenCalled();
  for (const operation of [snapshot, visible, clear, attach, lease]) expect(operation).not.toHaveBeenCalled();
  expect(f.service.ownedTerminalCount()).toBe(1); expect(f.processes[0].kill).not.toHaveBeenCalled();
  f.approved.mockReturnValue(true);
  expect(await f.send('/v1/request', JSON.stringify({ clientId: randomUUID(), method: 'getSnapshot', payload: {} })))
    .toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
});
it('requires helper-authenticated discovery and binds signatures to the exact discovery path and body', async () => {
  const f = await fixture();
  expect(await f.send('/v1/discover', '{}', {})).toEqual({ ok: false, error: { code: 'AUTH_FAILED', message: 'Unverified remote peer.', retryable: false } });
  expect(await f.send('/v1/discover', '{}', signedHeaders('wrong-secret', identity, '/v1/discover', '{}')))
    .toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  for (const [path, body] of [['/v1/request', '{}'], ['/v1/discover', '{"changed":true}']]) {
    expect(await f.send('/v1/discover', '{}', signedHeaders('discovery-test-secret', identity, path, body)))
      .toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  }
  expect(f.pending).not.toHaveBeenCalled(); expect(f.authorize).not.toHaveBeenCalled(); expect(f.approved).not.toHaveBeenCalled();
  for (const [path, method] of [['/v1/discover', 'GET'], ['/v1/discover?target=other', 'POST']]) {
    expect((await fetch(f.url + path, { method })).status).toBe(404);
  }
});
it('rate limits authenticated discovery without creating approval requests', async () => {
  const f = await fixture();
  const now = Date.now(); vi.spyOn(Date, 'now').mockReturnValue(now);
  for (let request = 0; request < 240; request++) {
    expect(await f.send('/v1/discover')).toEqual({ ok: true, value: { service: 'shellfox', protocolVersion: 1 } });
  }
  expect(await f.send('/v1/discover')).toEqual({ ok: false, error: { code: 'VALIDATION', message: 'Remote request rate exceeded.', retryable: true } });
  expect(await f.send('/v1/discover', '{}', {})).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  const peer = { ...identity, id: 'another-client-node' };
  expect(await f.send('/v1/discover', '{}', signedHeaders('discovery-test-secret', peer, '/v1/discover', '{}')))
    .toEqual({ ok: true, value: { service: 'shellfox', protocolVersion: 1 } });
  expect(f.pending).not.toHaveBeenCalled(); expect(f.authorize).not.toHaveBeenCalled(); expect(f.approved).not.toHaveBeenCalled();
});
it('keeps the legacy empty request signature validate-before-approval with no pending or terminal effects', async () => {
  const f = await fixture();
  const created = await f.service.createSession({ cwd: '/work', requestId: randomUUID() });
  if (!created.ok) throw new Error('fixture session missing');
  const snapshot = vi.spyOn(f.service, 'getSnapshot'), attach = vi.spyOn(f.service, 'attachTerminal');
  const clear = vi.spyOn(f.service, 'clearRemoteVisibleSessions'), lease = vi.spyOn(RemoteControlLeases.prototype, 'claim');
  for (const approved of [false, true]) {
    f.approved.mockReturnValue(approved);
    for (let attempt = 0; attempt < 4; attempt++) {
      expect(await f.send('/v1/request')).toEqual({
        ok: false, error: { code: 'VALIDATION', message: 'Remote request could not be completed.', retryable: true },
      });
    }
  }
  expect(f.pending).not.toHaveBeenCalled(); expect(f.authorize).not.toHaveBeenCalled();
  expect(f.approved).not.toHaveBeenCalled(); expect(f.withdraw).not.toHaveBeenCalled();
  for (const operation of [snapshot, attach, clear, lease]) expect(operation).not.toHaveBeenCalled();
  expect(f.service.ownedTerminalCount()).toBe(1); expect(f.processes[0].kill).not.toHaveBeenCalled();
  expect(await f.send('/v1/request', JSON.stringify({ clientId: randomUUID(), method: 'getSnapshot', payload: {} })))
    .toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
});
