import { afterEach, expect, it, vi } from 'vitest';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { FakeRemoteHelper, remoteFixture } from './test-fixtures';
import { RemoteService } from './service';
import { remoteNetworkPeerSchema, remotePeerSchema } from '../../shared/remote-schemas';
const cleanups: Array<() => Promise<void>> = [];
afterEach(async () => { for (const cleanup of cleanups.splice(0).reverse()) await cleanup(); vi.restoreAllMocks(); vi.useRealTimers(); });
it('discovers runtime hosts on the next poll, hides ordinary Tailscale devices and pushes results without pairing', async () => {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] });
  const network = new Map<string, FakeRemoteHelper>();
  network.set('phone', new FakeRemoteHelper('phone', network));
  const desktop = await remoteFixture(network, 'desktop'); cleanups.push(desktop.dispose);
  expect(desktop.remote.getState()).toMatchObject({ ok: true, value: { peers: [] } });
  const events: unknown[] = []; desktop.remote.subscribe(event => { if (event.type === 'state') events.push(event.state); });
  const host = await remoteFixture(network, 'new-host'); cleanups.push(host.dispose);
  await vi.advanceTimersByTimeAsync(3000);
  await vi.waitFor(() => expect(desktop.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ id: 'new-host', hostConfirmed: true, online: true })] } }));
  expect(events).toContainEqual(expect.objectContaining({ peers: [expect.objectContaining({ id: 'new-host', hostConfirmed: true })] }));
  expect(desktop.helper.requests.length).toBeGreaterThan(0);
  expect(desktop.helper.requests.every(request => request.path === '/v1/discover' && JSON.stringify(request.body) === '{}')).toBe(true);
  expect(host.remote.getState()).toMatchObject({ ok: true, value: { pending: [] } });
  await vi.waitFor(async () => expect(JSON.parse(await readFile(path.join(desktop.directory, 'discovered-hosts.json'), 'utf8'))).toEqual({ version: 1, ids: ['new-host'] }));
});
it('retains saved and active target rows through a transient peer omission with offline status', async () => {
  const network = new Map<string, FakeRemoteHelper>();
  const host = await remoteFixture(network, 'host'), desktop = await remoteFixture(network, 'desktop'); cleanups.push(host.dispose, desktop.dispose);
  await vi.waitFor(() => expect(desktop.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ hostConfirmed: true })] } }));
  await desktop.remote.connect({ peerId: 'host' });
  await vi.waitFor(() => expect(desktop.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ connection: 'awaiting-approval' })] } }));
  const command = desktop.helper.command.bind(desktop.helper);
  vi.spyOn(desktop.helper, 'command').mockImplementation((method, payload) => method === 'peers' ? Promise.resolve({ peers: [] }) : command(method, payload));
  await desktop.remote.login({});
  expect(desktop.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ id: 'host', hostConfirmed: true, online: false, remembered: true, connection: 'awaiting-approval' })] } });
  await desktop.remote.disconnect({ peerId: 'host' });
  expect(desktop.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ id: 'host', online: false, remembered: true, connection: 'disconnected' })] } });
});
it('loads confirmed saved hosts offline without promoting unverified legacy saved devices', async () => {
  const network = new Map<string, FakeRemoteHelper>();
  const host = await remoteFixture(network, 'host'), desktop = await remoteFixture(network, 'desktop'); cleanups.push(host.dispose, desktop.dispose);
  await vi.waitFor(() => expect(desktop.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ hostConfirmed: true })] } }));
  await desktop.remote.connect({ peerId: 'host' });
  await vi.waitFor(async () => expect(JSON.parse(await readFile(path.join(desktop.directory, 'discovered-hosts.json'), 'utf8'))).toEqual({ version: 1, ids: ['host'] }));
  await desktop.remote.dispose();
  const stored = await desktop.store.load();
  await desktop.store.save({ ...stored, peers: [...stored.peers, { id: 'old-phone', hostname: 'Old phone', addresses: ['100.64.0.9'], online: false }] });
  const helper = new FakeRemoteHelper('desktop', network), command = helper.command.bind(helper);
  vi.spyOn(helper, 'command').mockImplementation((method, payload) => method === 'peers' ? Promise.resolve({ peers: [] }) : command(method, payload));
  const restarted = new RemoteService(desktop.directory, desktop.store, helper, desktop.service, 'desktop'); cleanups.push(() => restarted.dispose());
  await restarted.initialize();
  expect(restarted.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ id: 'host', hostConfirmed: true, online: false }), expect.objectContaining({ id: 'old-phone', hostConfirmed: false, online: false, remembered: true })] } });
});
it('confirms and persists a successfully paired host even when discovery probes fail', async () => {
  const fetch = globalThis.fetch;
  vi.spyOn(globalThis, 'fetch').mockImplementation((input, init) => typeof input === 'string' && input.endsWith('/v1/discover')
    ? Promise.resolve(new Response(JSON.stringify({ service: 'ordinary-server' }), { status: 200 })) : fetch(input, init));
  const network = new Map<string, FakeRemoteHelper>();
  const host = await remoteFixture(network, 'host'), desktop = await remoteFixture(network, 'desktop'); cleanups.push(host.dispose, desktop.dispose);
  await desktop.remote.connect({ peerId: 'host' });
  await vi.waitFor(() => expect(host.remote.getState()).toMatchObject({ ok: true, value: { pending: [expect.objectContaining({ id: 'desktop' })] } }));
  expect(desktop.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ id: 'host', hostConfirmed: false })] } });
  await host.remote.approve({ peerId: 'desktop' });
  await vi.waitFor(() => expect(desktop.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ id: 'host', hostConfirmed: true, online: true, connection: 'connected' })] } }), { timeout: 4000 });
  await vi.waitFor(async () => expect(JSON.parse(await readFile(path.join(desktop.directory, 'discovered-hosts.json'), 'utf8'))).toEqual({ version: 1, ids: ['host'] }));
});
it('stops retrying a legacy shared-control host and permits an explicit retry after upgrade', async () => {
  const fetch = globalThis.fetch;
  vi.spyOn(globalThis, 'fetch').mockImplementation((input, init) => typeof input === 'string' && input.endsWith('/v1/pair')
    ? Promise.resolve(new Response(JSON.stringify({ ok: true, value: { paired: true } }), { status: 200 })) : fetch(input, init));
  const network = new Map<string, FakeRemoteHelper>();
  const host = await remoteFixture(network, 'host'), desktop = await remoteFixture(network, 'desktop'); cleanups.push(host.dispose, desktop.dispose);
  await desktop.remote.connect({ peerId: 'host' });
  await vi.waitFor(() => expect(desktop.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ id: 'host', connection: 'disconnected', lastError: expect.objectContaining({ code: 'UNSUPPORTED', retryable: false }) })] } }));
  const attempts = () => desktop.helper.requests.filter(request => request.path === '/v1/pair');
  await new Promise(resolve => setTimeout(resolve, 1100)); expect(attempts()).toHaveLength(1);
  await desktop.remote.connect({ peerId: 'host' });
  await vi.waitFor(() => expect(attempts()).toHaveLength(2));
  expect(attempts()[0].body).not.toEqual(attempts()[1].body);
});
it('accepts older desktop peer states without changing the mobile native peer/discovery schema', () => {
  const peer = { id: 'host', hostname: 'host', addresses: ['100.64.0.1'], online: true };
  const legacy = { ...peer, connection: 'disconnected', approved: false, remembered: false };
  expect(remotePeerSchema.parse(legacy)).not.toHaveProperty('hostConfirmed');
  expect(remotePeerSchema.parse({ ...legacy, hostConfirmed: true }).hostConfirmed).toBe(true);
  expect(remotePeerSchema.safeParse({ ...legacy, hostConfirmed: 'true' }).success).toBe(false);
  expect(remoteNetworkPeerSchema.parse(peer)).toEqual(peer);
  expect(remoteNetworkPeerSchema.safeParse({ ...peer, hostConfirmed: true }).success).toBe(false);
});
