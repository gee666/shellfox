import { afterEach, expect, it, vi } from 'vitest';
import { z } from 'zod';
import { failure, success } from '../../../src/shared/contracts';
import { REMOTE_APPROVAL_TTL_MS, RemoteApprovals } from '../../../src/main/remote/approvals';
import { RemoteClient } from './client';
import { attachment, deferred, flush, ids, TestTransport } from './test-fixtures';

const cleanups: Array<() => void> = [];
afterEach(() => { cleanups.splice(0).reverse().forEach(cleanup => cleanup()); vi.useRealTimers(); });
function setup(transport = new TestTransport()) {
  vi.useFakeTimers();
  const createClientId = vi.fn().mockReturnValueOnce(ids.client).mockReturnValueOnce(ids.nextClient);
  const client = new RemoteClient('host-1', transport, { createClientId });
  cleanups.push(() => client.stop());
  return { client, transport };
}
it('finishes an in-flight pair while suspended without withdrawing, retrying, or polling', async () => {
  const { client, transport } = setup(), pair = deferred<string>(); transport.pair = () => pair.promise;
  client.connect(); await flush(); client.suspend();
  pair.resolve(JSON.stringify(failure('APPROVAL_REQUIRED', 'Approve on host.', true))); await flush();
  expect(client.getState().status).toBe('awaiting-approval');
  await vi.advanceTimersByTimeAsync(180_000);
  expect(transport.calls('/v1/pair')).toHaveLength(1);
  expect(transport.calls('/v1/disconnect')).toHaveLength(0); expect(transport.polls).toHaveLength(0);
  client.connect(); await flush();
  expect(transport.calls('/v1/pair')).toEqual(Array(2).fill(JSON.stringify({ clientId: ids.client })));
  client.stop(); await flush();
  expect(transport.calls('/v1/disconnect').every(body => JSON.parse(body).clientId === ids.client)).toBe(true);
});
it('does not stack native pairs across rapid screen changes and explicit cancel still fences a late pair', async () => {
  const { client, transport } = setup(), pair = deferred<string>(); transport.pair = () => pair.promise;
  client.connect(); await flush();
  for (let i = 0; i < 100; i++) { client.suspend(); client.connect(); await flush(); }
  expect(transport.calls('/v1/pair')).toHaveLength(1);
  client.suspend(); client.stop(); await flush();
  const releases = transport.calls('/v1/disconnect').length;
  pair.resolve(JSON.stringify(failure('APPROVAL_REQUIRED', 'Approve on host.', true))); await flush();
  expect(transport.calls('/v1/disconnect').length).toBeGreaterThan(releases);
  expect(client.getState().status).toBe('disconnected');
  transport.pair = async () => JSON.stringify(failure('APPROVAL_REQUIRED', 'Approve on host.', true));
  client.connect(); await flush();
  expect(transport.calls('/v1/pair').at(-1)).toBe(JSON.stringify({ clientId: ids.nextClient }));
});
it('drains the native event poll and operations before resuming instead of abandoning them on each screen change', async () => {
  const { client, transport } = setup(), write = deferred<string>();
  client.connect(); await flush();
  transport.operation = () => write.promise;
  const input = client.writeTerminal({ tabId: ids.tab, generation: ids.generation, controlEpoch: 1, data: 'x' }); await flush();
  for (let i = 0; i < 100; i++) { client.suspend(); client.connect(); await flush(); }
  expect(transport.calls('/v1/pair')).toHaveLength(1); expect(transport.polls).toHaveLength(1);
  expect(transport.calls('/v1/request')).toHaveLength(1); expect(transport.calls('/v1/disconnect')).toHaveLength(0);
  transport.events([]); await flush();
  expect(transport.calls('/v1/pair')).toHaveLength(1);
  write.resolve(JSON.stringify(success({ written: true }))); await input; await flush();
  expect(client.getState().status).toBe('connected');
  expect(transport.calls('/v1/pair')).toHaveLength(2); expect(transport.polls).toHaveLength(2);
});
it.each(['owned', 'blocked'] as const)('detaches a background %s terminal viewer without cancelling the approval attempt', async control => {
  const { client, transport } = setup();
  transport.operation = async body => {
    const input = JSON.parse(body);
    if (input.method === 'attachTerminal') return JSON.stringify(success(control === 'owned' ? attachment
      : { tabId: ids.tab, sessionId: ids.session, generation: ids.generation, control: 'blocked', controlEpoch: 2 }));
    return JSON.stringify(success({ detached: true }));
  };
  client.connect(); await flush(); await client.attachTerminal({ tabId: ids.tab });
  const terminal = vi.fn(); client.subscribeTerminal(terminal);
  client.suspend(); await flush();
  expect(terminal).toHaveBeenCalledExactlyOnceWith({ type: 'control', tabId: ids.tab, generation: ids.generation, control: 'blocked', controlEpoch: control === 'owned' ? 1 : 2 });
  expect(transport.calls('/v1/request').map(body => JSON.parse(body))).toEqual([
    expect.objectContaining({ method: 'attachTerminal' }),
    expect.objectContaining({ method: 'detachTerminal', payload: { tabId: ids.tab, generation: ids.generation } }),
  ]);
  expect(transport.calls('/v1/disconnect')).toHaveLength(0); expect(client.getState().status).toBe('disconnected');
});
it('retains a delivered host request beyond two minutes with no heartbeat and withdraws the original UUID after resume', async () => {
  vi.useFakeTimers();
  let now = 0;
  const policy = new RemoteApprovals(() => false, () => {}, () => now), calls: string[] = [];
  const attempt = z.object({ clientId: z.uuid() });
  const client = new RemoteClient('host-1', { request: async (_peer, path, body) => {
    calls.push(path); const { clientId } = attempt.parse(JSON.parse(body));
    if (path === '/v1/pair') return JSON.stringify({ ok: false, error: policy.authorize({ id: 'mobile', hostname: 'phone' }, clientId) });
    if (path === '/v1/disconnect') { policy.withdraw('mobile', clientId); return JSON.stringify(success({ disconnected: true })); }
    throw new Error('Unexpected background traffic');
  } }, { createClientId: () => ids.client });
  cleanups.push(() => client.stop()); client.connect(); await flush(); client.suspend();
  now = 180_000; await vi.advanceTimersByTimeAsync(now); policy.expire();
  expect(policy.pending.has('mobile')).toBe(true); expect(calls).toEqual(['/v1/pair']);
  client.connect(); await flush(); client.stop(); await flush();
  expect(policy.pending.size).toBe(0);
  expect(policy.authorize({ id: 'mobile', hostname: 'phone' }, ids.client)?.code).toBe('ACCESS_DENIED');
  expect(policy.authorize({ id: 'mobile', hostname: 'phone' }, ids.nextClient)?.code).toBe('APPROVAL_REQUIRED');
  now += REMOTE_APPROVAL_TTL_MS + 1; policy.expire(); expect(policy.pending.size).toBe(0);
});
