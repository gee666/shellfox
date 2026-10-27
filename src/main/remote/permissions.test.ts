import { afterEach, expect, it } from 'vitest';
import { vi } from 'vitest';
import { z } from 'zod';
import { failure } from '../../shared/contracts';
import type { RemoteEvent } from '../../shared/remote-contracts';
import { FakeRemoteHelper, remoteFixture, signedHeaders } from './test-fixtures';
import { RemoteApprovals } from './approvals';

const cleanups: Array<() => Promise<void>> = [];
afterEach(async () => { for (const cleanup of cleanups.splice(0).reverse()) await cleanup(); });
async function networkFixture() {
  const network = new Map<string, FakeRemoteHelper>();
  const host = await remoteFixture(network, 'host'), client = await remoteFixture(network, 'client');
  cleanups.push(host.dispose, client.dispose); return { host, client };
}
const attempt = z.object({ clientId: z.uuid() });
// Approval retries wait one second before their next HTTP attempt.
const approvalWait = { timeout: 5000 };
it('exposes waiting/rejected states, stops rejected retries, and requests again with a fresh attempt', async () => {
  const { host, client } = await networkFixture(); await host.create();
  const view = client.remote.createView('cli'); cleanups.push(() => view.dispose());
  await client.remote.connect({ peerId: 'host' });
  await vi.waitFor(() => expect(client.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ connection: 'awaiting-approval', lastError: expect.objectContaining({ code: 'APPROVAL_REQUIRED', retryable: true }) })] } }));
  expect(await client.remote.request({ peerId: 'host', method: 'getSnapshot', payload: {} })).toMatchObject({ ok: false, error: { code: 'APPROVAL_REQUIRED' } });
  expect(await view.request({ peerId: 'host', method: 'getSnapshot', payload: {} })).toMatchObject({ ok: false, error: { code: 'APPROVAL_REQUIRED' } });
  const first = attempt.parse(client.helper.requests.find(request => request.path === '/v1/pair')?.body).clientId;
  expect(await host.remote.reject({ peerId: 'client' })).toMatchObject({ ok: true, value: { pending: [] } });
  await vi.waitFor(() => expect(client.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ connection: 'rejected', lastError: expect.objectContaining({ code: 'ACCESS_DENIED', retryable: false }) })] } }), { timeout: 2000 });
  const requests = client.helper.requests.length; await new Promise(resolve => setTimeout(resolve, 2100));
  expect(client.helper.requests).toHaveLength(requests); expect(host.remote.getState()).toMatchObject({ ok: true, value: { pending: [] } });
  expect(await view.request({ peerId: 'host', method: 'getSnapshot', payload: {} })).toMatchObject({ ok: false, error: { code: 'ACCESS_DENIED' } });
  await client.remote.connect({ peerId: 'host' });
  await vi.waitFor(() => expect(host.remote.getState()).toMatchObject({ ok: true, value: { pending: [expect.objectContaining({ id: 'client' })] } }));
  const pairs = client.helper.requests.filter(request => request.path === '/v1/pair');
  expect(attempt.parse(pairs.at(-1)?.body).clientId).not.toBe(first);
  await host.remote.approve({ peerId: 'client' });
  await vi.waitFor(() => expect(client.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ connection: 'connected', lastError: null })] } }), approvalWait);
  expect(await view.request({ peerId: 'host', method: 'getSnapshot', payload: {} })).toMatchObject({ ok: true });
}, 10_000);
it('withdraws a cancelled pending attempt, tombstones late retries and accepts a fresh request UUID', async () => {
  const { host, client } = await networkFixture(); await client.remote.connect({ peerId: 'host' });
  await vi.waitFor(() => expect(client.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ connection: 'awaiting-approval' })] } }));
  const old = attempt.parse(client.helper.requests.find(request => request.path === '/v1/pair')?.body).clientId;
  await client.remote.disconnect({ peerId: 'host' });
  await vi.waitFor(() => expect(host.remote.getState()).toMatchObject({ ok: true, value: { pending: [] } }));
  const body = JSON.stringify({ clientId: old }), route = '/v1/pair';
  const response = await fetch(host.helper.backendUrl + route, { method: 'POST', body, headers: signedHeaders(host.helper.secret, { id: 'client', hostname: 'client' }, route, body) });
  expect(await response.json()).toMatchObject({ ok: false, error: { code: 'ACCESS_DENIED', retryable: false } });
  expect(host.remote.getState()).toMatchObject({ ok: true, value: { pending: [] } });
  await client.remote.connect({ peerId: 'host' });
  await vi.waitFor(() => expect(host.remote.getState()).toMatchObject({ ok: true, value: { pending: [expect.objectContaining({ id: 'client' })] } }));
  const pairs = client.helper.requests.filter(request => request.path === '/v1/pair'); expect(attempt.parse(pairs.at(-1)?.body).clientId).not.toBe(old);
});
it('revokes every active secondary view with a typed denial and never kills host shells', async () => {
  const { host, client } = await networkFixture(); const created = await host.create(); if (!created.ok) throw new Error('Create failed');
  await client.remote.connect({ peerId: 'host' });
  await vi.waitFor(() => expect(host.remote.getState()).toMatchObject({ ok: true, value: { pending: [expect.anything()] } })); await host.remote.approve({ peerId: 'client' });
  await vi.waitFor(() => expect(client.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ connection: 'connected' })] } }), approvalWait);
  const a = client.remote.createView('a'), b = client.remote.createView('b'); cleanups.push(() => a.dispose(), () => b.dispose());
  const events: RemoteEvent[] = []; a.subscribe(event => events.push(event)); b.subscribe(event => events.push(event));
  for (const view of [a, b]) expect(await view.request({ peerId: 'host', method: 'attachTerminal', payload: { tabId: created.value.tabs[0].id } })).toMatchObject({ ok: true });
  await host.remote.revoke({ peerId: 'client' });
  await vi.waitFor(() => expect(client.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ connection: 'rejected' })] } }));
  await vi.waitFor(() => expect(events.filter(event => event.type === 'terminal' && event.event.type === 'error' && event.event.error.code === 'ACCESS_DENIED')).toHaveLength(2));
  expect(await b.request({ peerId: 'host', method: 'getSnapshot', payload: {} })).toMatchObject({ ok: false, error: { code: 'ACCESS_DENIED' } });
  expect(host.processes[0].kill).not.toHaveBeenCalled();
});
it('interrupts a long event poll when a secondary view reports approval is required', async () => {
  const { host, client } = await networkFixture(); await client.remote.connect({ peerId: 'host' });
  await vi.waitFor(() => expect(host.remote.getState()).toMatchObject({ ok: true, value: { pending: [expect.anything()] } })); await host.remote.approve({ peerId: 'client' });
  await vi.waitFor(() => expect(client.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ connection: 'connected' })] } }), approvalWait);
  const states: string[] = []; client.remote.subscribe(event => { if (event.type === 'state') states.push(event.state.peers.find(peer => peer.id === 'host')?.connection ?? 'missing'); });
  const view = client.remote.createView('approval-view'); cleanups.push(() => view.dispose());
  // Pair this view before injecting the operation failure. Pairing also reads
  // profiles to publish activity, which must not consume the one-shot denial.
  expect(await view.request({ peerId: 'host', method: 'getSnapshot', payload: {} })).toMatchObject({ ok: true });
  const profiles = vi.spyOn(host.service, 'getTerminalProfiles').mockReturnValueOnce(failure('APPROVAL_REQUIRED', 'Host approval is pending.', true));
  try {
    expect(await view.request({ peerId: 'host', method: 'getTerminalProfiles', payload: {} })).toMatchObject({ ok: false, error: { code: 'APPROVAL_REQUIRED' } });
    expect(profiles).toHaveBeenCalledOnce();
  } finally { profiles.mockRestore(); }
  expect(states).toContain('awaiting-approval');
  await vi.waitFor(() => expect(states.at(-1)).toBe('connected'), approvalWait);
});
it('bounds pending notifications and preserves old denial decisions after a different attempt is approved', () => {
  let allowed = false; const changed = vi.fn(), policy = new RemoteApprovals(() => allowed, changed);
  expect(policy.authorize({ id: 'peer', hostname: 'peer' }, 'first')?.code).toBe('APPROVAL_REQUIRED');
  for (let i = 0; i < 100; i++) policy.authorize({ id: 'peer', hostname: 'peer' }, 'first'); expect(changed).toHaveBeenCalledOnce();
  policy.deny('peer'); expect(policy.authorize({ id: 'peer', hostname: 'peer' }, 'first')?.code).toBe('ACCESS_DENIED');
  policy.authorize({ id: 'peer', hostname: 'peer' }, 'second'); allowed = true; policy.approve('peer');
  expect(policy.authorize({ id: 'peer', hostname: 'peer' }, 'first')?.code).toBe('ACCESS_DENIED');
  expect(policy.authorize({ id: 'peer', hostname: 'peer' }, 'second')).toBeNull();
  allowed = false; for (let i = 0; i < 200; i++) policy.authorize({ id: 'peer-' + i, hostname: 'peer' }, 'attempt');
  expect(policy.pending.size).toBe(100);
});
