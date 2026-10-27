import { afterEach, expect, it, vi } from 'vitest';
import { createServer, type Server } from 'node:http';
import { DISCOVERY_NEGATIVE_TTL, DISCOVERY_POSITIVE_TTL, RemoteDiscovery, probeShellfoxHost } from './discovery';
const peers = (ids: string[], online = true) => ids.map(id => ({ id, hostname: id, addresses: ['100.64.0.1'], online }));
const signature = { ok: true, value: { service: 'shellfox', protocolVersion: 1 } };
const legacy = { ok: false, error: { code: 'VALIDATION', message: 'Remote request could not be completed.', retryable: true } };
const servers: Server[] = [];
afterEach(async () => { vi.restoreAllMocks(); for (const server of servers.splice(0)) { server.closeAllConnections(); await new Promise<void>(resolve => server.close(() => resolve())); } });
async function proxy(reply: (path: string) => { status: number; body: unknown }) {
  const requests: unknown[] = [];
  const server = createServer((request, response) => { void (async () => {
    expect(request.url).toBe('/proxy'); expect(request.headers.authorization).toBe('Bearer private-secret');
    const chunks: Buffer[] = []; for await (const chunk of request) chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
    const input: unknown = JSON.parse(Buffer.concat(chunks).toString('utf8')); requests.push(input);
    if (input === null || typeof input !== 'object' || !('path' in input) || typeof input.path !== 'string') throw new Error('Invalid test envelope');
    const result = reply(input.path); response.writeHead(result.status, { 'Content-Type': 'application/json' }); response.end(JSON.stringify(result.body));
  })().catch(() => { response.writeHead(500); response.end(); }); });
  servers.push(server); await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const address = server.address(); if (!address || typeof address === 'string') throw new Error('No proxy');
  return { url: `http://127.0.0.1:${address.port}`, requests };
}
it.each([
  ['current host', signature, true], ['ordinary device', { name: 'web server' }, false],
  ['wrong protocol', { ok: true, value: { service: 'shellfox', protocolVersion: 2 } }, false],
  ['extra fields', { ...signature, clientId: 'not-discovery' }, false],
  ['oversized', { text: 'x'.repeat(4097) }, false],
] as const)('requires the exact Shellfox signature for %s without pairing', async (_name, body, expected) => {
  const f = await proxy(() => ({ status: 200, body }));
  expect(await probeShellfoxHost(f.url, 'private-secret', 'peer-1', new AbortController().signal)).toBe(expected);
  expect(f.requests).toEqual([{ peerId: 'peer-1', path: '/v1/discover', body: {} }]);
});
it('uses the same identity-free legacy validation fallback as mobile only after 404', async () => {
  const f = await proxy(path => path === '/v1/discover' ? { status: 404, body: {} } : { status: 200, body: legacy });
  expect(await probeShellfoxHost(f.url, 'private-secret', 'peer-1', new AbortController().signal)).toBe(true);
  expect(f.requests).toEqual([{ peerId: 'peer-1', path: '/v1/discover', body: {} }, { peerId: 'peer-1', path: '/v1/request', body: {} }]);
  const denied = await proxy(() => ({ status: 403, body: signature }));
  expect(await probeShellfoxHost(denied.url, 'private-secret', 'peer-1', new AbortController().signal)).toBe(false);
  expect(denied.requests).toHaveLength(1);
});
it('shares one two-second deadline across discovery and the legacy fallback', async () => {
  let requests = 0;
  const server = createServer((_request, response) => {
    const first = ++requests === 1;
    const timer = setTimeout(() => { response.writeHead(first ? 404 : 200, { 'Content-Type': 'application/json' }); response.end(JSON.stringify(first ? {} : legacy)); }, 1200);
    response.once('close', () => clearTimeout(timer));
  });
  servers.push(server); await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const address = server.address(); if (!address || typeof address === 'string') throw new Error('No proxy');
  expect(await probeShellfoxHost(`http://127.0.0.1:${address.port}`, 'private-secret', 'peer-1', new AbortController().signal)).toBe(false);
  expect(requests).toBe(2);
});
it('caches positive and negative probes separately and retains known hosts offline', async () => {
  let now = 0;
  const probe = vi.fn(async (id: string) => id === 'host'), changed = vi.fn();
  const discovery = new RemoteDiscovery(probe, changed, () => now);
  discovery.update(peers(['host', 'ordinary']));
  await vi.waitFor(() => expect(changed).toHaveBeenCalledTimes(2));
  expect(discovery.peers().map(peer => peer.id)).toEqual(['host']);
  discovery.update(peers(['host', 'ordinary'])); expect(probe).toHaveBeenCalledTimes(2);
  now = DISCOVERY_NEGATIVE_TTL; discovery.update(peers(['host', 'ordinary']));
  await vi.waitFor(() => expect(changed).toHaveBeenCalledTimes(3));
  expect(probe).toHaveBeenCalledTimes(3); expect(probe).toHaveBeenLastCalledWith('ordinary', expect.anything());
  now = DISCOVERY_POSITIVE_TTL; discovery.update(peers(['host', 'ordinary']));
  await vi.waitFor(() => expect(changed).toHaveBeenCalledTimes(5));
  expect(probe).toHaveBeenCalledTimes(5);
  discovery.update(peers(['host', 'ordinary'], false));
  expect(discovery.peers()).toEqual(peers(['host'], false)); expect(probe).toHaveBeenCalledTimes(5);
});
it('discovers a device that starts Shellfox after a negative TTL without restarting the service', async () => {
  let now = 0, hosting = false;
  const changed = vi.fn(), discovery = new RemoteDiscovery(async () => hosting, changed, () => now);
  discovery.update(peers(['new-device'])); await vi.waitFor(() => expect(changed).toHaveBeenCalledOnce());
  expect(discovery.peers()).toEqual([]);
  hosting = true; now = DISCOVERY_NEGATIVE_TTL;
  discovery.update(peers(['new-device'])); await vi.waitFor(() => expect(discovery.peers()).toEqual(peers(['new-device'])));
});
it('bounds discovery to four slots, coalesces refreshes and accepts runtime peer-map additions', async () => {
  const finish: Array<(value: boolean) => void> = [];
  const probe = vi.fn(() => new Promise<boolean>(resolve => { finish.push(resolve); }));
  const changed = vi.fn(), discovery = new RemoteDiscovery(probe, changed);
  discovery.update(peers(['a', 'b', 'c', 'd', 'e'])); discovery.update(peers(['a', 'b', 'c', 'd', 'e', 'new-device']));
  expect(probe).toHaveBeenCalledTimes(4);
  finish[0](true); await vi.waitFor(() => expect(probe).toHaveBeenCalledTimes(5));
  expect(probe).toHaveBeenLastCalledWith('new-device', expect.anything());
  for (const done of finish) done(true);
  await vi.waitFor(() => expect(probe).toHaveBeenCalledTimes(6)); finish.at(-1)?.(true);
  await vi.waitFor(() => expect(discovery.peers()).toHaveLength(6));
  expect(changed).toHaveBeenCalledTimes(6);
});
it('fences late probes across disable/logout and removes revoked peers from the visible list', async () => {
  let finish: (value: boolean) => void = () => {};
  const changed = vi.fn(), discovery = new RemoteDiscovery(() => new Promise<boolean>(resolve => { finish = resolve; }), changed);
  discovery.update(peers(['removed'])); discovery.clear(); finish(true);
  await new Promise<void>(resolve => setImmediate(resolve)); expect(discovery.peers()).toEqual([]); expect(changed).not.toHaveBeenCalled();
  const confirmed = new RemoteDiscovery(async () => true, changed);
  confirmed.update(peers(['host'])); await vi.waitFor(() => expect(confirmed.peers()).toHaveLength(1));
  confirmed.update([]); expect(confirmed.peers()).toEqual([]);
});
