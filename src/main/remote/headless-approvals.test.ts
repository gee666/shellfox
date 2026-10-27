import { randomUUID } from 'node:crypto';
import { z } from 'zod';
import { afterEach, expect, it, vi } from 'vitest';
import { HostClient } from '../../host/client';
import { startHostRpc } from '../../host/rpc';
import type { RemoteEvent } from '../../shared/remote-contracts';
import { FakeRemoteHelper, remoteFixture, signedHeaders } from './test-fixtures';

const cleanups: Array<() => Promise<void>> = [];
afterEach(async () => { for (const cleanup of cleanups.splice(0).reverse()) await cleanup(); });
async function fixture() {
  const network = new Map<string, FakeRemoteHelper>();
  const host = await remoteFixture(network, 'host'), client = await remoteFixture(network, 'client');
  cleanups.push(host.dispose, client.dispose);
  for (const node of [host, client]) {
    const rpc = await startHostRpc({ userData: node.directory, mode: 'headless', service: node.service, remote: node.remote, shutdown: async () => {} });
    cleanups.push(() => rpc.close());
  }
  const hostCli = await HostClient.connect(host.directory), clientCli = await HostClient.connect(client.directory);
  cleanups.push(() => hostCli.close(), () => clientCli.close());
  const send = async (peerId: string, clientId: string, route = '/v1/pair') => {
    const body = JSON.stringify({ clientId });
    const response = await fetch(host.helper.backendUrl + route, { method: 'POST', body,
      headers: signedHeaders(host.helper.secret, { id: peerId, hostname: peerId }, route, body) });
    return response.json();
  };
  return { host, client, hostCli, clientCli, send };
}
const attemptSchema = z.object({ clientId: z.uuid() });
const waiting = { ok: true, value: { peers: [expect.objectContaining({ connection: 'awaiting-approval' })] } };
const pending = { ok: true, value: { pending: [expect.objectContaining({ id: 'client' })] } };

it('pushes a headless approval request before reporting waiting, survives CLI exit and withdraws only the cancelled UUID', async () => {
  const { host, client, hostCli, clientCli, send } = await fixture();
  const hostEvents: RemoteEvent[] = []; let waitingWithoutRequest = false;
  hostCli.remote.subscribe(event => hostEvents.push(event));
  clientCli.remote.subscribe(event => {
    if (event.type === 'state' && event.state.peers.some(peer => peer.connection === 'awaiting-approval')) {
      const state = host.remote.getState();
      if (!state.ok || !state.value.pending.some(peer => peer.id === 'client')) waitingWithoutRequest = true;
    }
  });
  await clientCli.remote.connect({ peerId: 'host' });
  await vi.waitFor(() => expect(client.remote.getState()).toMatchObject(waiting));
  await vi.waitFor(() => expect(hostEvents).toContainEqual(expect.objectContaining({ type: 'state', state: expect.objectContaining({ pending: [expect.objectContaining({ id: 'client' })] }) })));
  const first = attemptSchema.parse(client.helper.requests.find(request => request.path === '/v1/pair')?.body);
  await clientCli.close();
  await new Promise(resolve => setTimeout(resolve, 1100));
  expect(host.remote.getState()).toMatchObject(pending);
  expect(client.helper.requests.filter(request => request.path === '/v1/pair').every(request => attemptSchema.parse(request.body).clientId === first.clientId)).toBe(true);
  const nextCli = await HostClient.connect(client.directory); cleanups.push(() => nextCli.close());
  expect(await nextCli.remote.getState()).toMatchObject(waiting);
  await nextCli.remote.disconnect({ peerId: 'host' });
  await vi.waitFor(() => expect(host.remote.getState()).toMatchObject({ ok: true, value: { pending: [] } }));
  await nextCli.remote.connect({ peerId: 'host' });
  await vi.waitFor(() => expect(client.remote.getState()).toMatchObject(waiting));
  const pairs = client.helper.requests.filter(request => request.path === '/v1/pair');
  const oldId = first.clientId;
  const freshId = attemptSchema.parse(pairs.at(-1)?.body).clientId;
  expect(freshId).not.toBe(oldId);
  await send('client', oldId, '/v1/disconnect');
  expect(await send('client', oldId)).toMatchObject({ ok: false, error: { code: 'ACCESS_DENIED' } });
  expect(host.remote.getState()).toMatchObject(pending);
  await hostCli.remote.approve({ peerId: 'client' });
  await vi.waitFor(() => expect(client.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ connection: 'connected' })] } }), { timeout: 3000 });
  expect(await nextCli.remote.request({ peerId: 'host', method: 'getSnapshot', payload: {} })).toMatchObject({ ok: true });
  expect(waitingWithoutRequest).toBe(false);
});

it('does not claim a headless client is awaiting approval when the host queue is full, and retries the same UUID once space opens', async () => {
  const { host, client, clientCli, send } = await fixture();
  const attempts = Array.from({ length: 100 }, () => randomUUID());
  for (const [index, id] of attempts.entries()) expect(await send(`queued-${index}`, id)).toMatchObject({ ok: false, error: { code: 'APPROVAL_REQUIRED' } });
  await clientCli.remote.connect({ peerId: 'host' });
  await vi.waitFor(() => expect(client.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ connection: 'reconnecting', lastError: expect.objectContaining({ code: 'VALIDATION', retryable: true }) })] } }));
  const blocked = client.helper.requests.find(request => request.path === '/v1/pair')?.body;
  expect(host.remote.getState()).toMatchObject({ ok: true, value: { pending: expect.not.arrayContaining([expect.objectContaining({ id: 'client' })]) } });
  await send('queued-0', attempts[0], '/v1/disconnect');
  await vi.waitFor(() => expect(client.remote.getState()).toMatchObject(waiting), { timeout: 3000 });
  expect(host.remote.getState()).toMatchObject({ ok: true, value: { pending: expect.arrayContaining([expect.objectContaining({ id: 'client' })]) } });
  const pairs = client.helper.requests.filter(request => request.path === '/v1/pair');
  expect(pairs.at(-1)?.body).toEqual(blocked);
});
