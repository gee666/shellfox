import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { failure, success } from '../../../src/shared/contracts';
import { RemoteClient } from './client';
import { deferred, flush, ids, TestTransport } from './test-fixtures';

let transport: TestTransport;
let client: RemoteClient;
beforeEach(() => {
  vi.useFakeTimers(); transport = new TestTransport();
  client = new RemoteClient('host-1', transport, { createClientId: () => ids.client });
});
afterEach(() => { client.stop(); vi.useRealTimers(); });

it('pairs, reports state and delivers typed events through one long poll', async () => {
  const state = vi.fn(), event = vi.fn();
  const offState = client.subscribeState(state), offEvent = client.subscribe(event);
  client.connect(); client.connect(); await flush();
  expect(client.getState()).toEqual({ peerId: 'host-1', status: 'connected', lastError: null, connectionId: `${ids.client}:1` });
  expect(transport.calls('/v1/pair')).toEqual([JSON.stringify({ clientId: ids.client })]);
  expect(transport.polls).toHaveLength(1);
  const changed = { type: 'changed', event: { revision: 1, reason: 'sessions' } } as const;
  transport.events([changed]); await flush();
  expect(event).toHaveBeenCalledWith(changed);
  expect(transport.polls).toHaveLength(2);
  expect(state.mock.calls.map(call => call[0].status)).toEqual(['connecting', 'connected']);
  offState(); offEvent(); transport.events([changed]); await flush();
  expect(event).toHaveBeenCalledTimes(1);
});

it('retries pending approval with the same attempt ID, without withdrawing it', async () => {
  let attempts = 0;
  transport.pair = async () => JSON.stringify(++attempts === 1 ? failure('APPROVAL_REQUIRED', 'Approve on host.', true) : success({ paired: true }));
  client.connect(); await flush();
  expect(client.getState()).toMatchObject({ status: 'awaiting-approval', lastError: { code: 'APPROVAL_REQUIRED' } });
  expect(transport.calls('/v1/disconnect')).toHaveLength(0);
  await vi.advanceTimersByTimeAsync(1000);
  expect(client.getState().status).toBe('connected');
  expect(transport.calls('/v1/pair')).toEqual(Array(2).fill(JSON.stringify({ clientId: ids.client })));
});

it('does not retry a rejected attempt and uses a fresh UUID on explicit connect', async () => {
  const createClientId = vi.fn().mockReturnValueOnce(ids.client).mockReturnValueOnce(ids.nextClient);
  client = new RemoteClient('host-1', transport, { createClientId });
  transport.pair = async () => JSON.stringify(failure('ACCESS_DENIED', 'Declined.', false));
  client.connect(); await flush();
  expect(client.getState().status).toBe('rejected');
  await vi.advanceTimersByTimeAsync(60_000);
  expect(transport.calls('/v1/pair')).toHaveLength(1);
  transport.pair = async () => JSON.stringify(success({ paired: true }));
  client.connect(); await flush();
  expect(client.getState().status).toBe('connected');
  expect(JSON.parse(transport.calls('/v1/pair')[1])).toEqual({ clientId: ids.nextClient });
});

it('backs off unavailable hosts, stops timers, and reconnects with a new epoch', async () => {
  let attempts = 0;
  transport.pair = async () => { if (++attempts < 3) throw new Error('offline'); return JSON.stringify(success({ paired: true })); };
  client.connect(); await flush();
  expect(client.getState().status).toBe('reconnecting');
  await vi.advanceTimersByTimeAsync(999); expect(attempts).toBe(1);
  await vi.advanceTimersByTimeAsync(1); expect(attempts).toBe(2);
  await vi.advanceTimersByTimeAsync(1999); expect(attempts).toBe(2);
  await vi.advanceTimersByTimeAsync(1); expect(attempts).toBe(3);
  const epoch = client.getState().connectionId;
  transport.polls[0].reject(new Error('network lost')); await flush();
  await vi.advanceTimersByTimeAsync(1000);
  expect(client.getState().status).toBe('connected');
  expect(client.getState().connectionId).not.toBe(epoch);
  client.stop(); await vi.advanceTimersByTimeAsync(60_000);
  expect(attempts).toBe(4);
  expect(client.getState().status).toBe('disconnected');
});

it('ignores late poll replies after stop and disconnects host views', async () => {
  const event = vi.fn(); client.subscribe(event);
  client.connect(); await flush(); client.stop();
  transport.events([{ type: 'changed', event: { revision: 1, reason: 'sessions' } }]); await flush();
  expect(event).not.toHaveBeenCalled();
  expect(client.getState().status).toBe('disconnected');
  expect(transport.calls('/v1/disconnect').length).toBeGreaterThan(0);
  expect(transport.polls).toHaveLength(1);
});

it('withdraws a late native pair after stop, even if the first disconnect arrived too early', async () => {
  const pair = deferred<string>(); transport.pair = () => pair.promise;
  client.connect(); await flush(); client.stop(); await flush();
  const releases = transport.calls('/v1/disconnect').length;
  pair.resolve(JSON.stringify(success({ paired: true }))); await flush();
  expect(transport.calls('/v1/disconnect').length).toBeGreaterThan(releases);
  expect(client.getState().status).toBe('disconnected');
  expect(transport.polls).toHaveLength(0);
});

it('interrupts a pending event poll on operation-level approval or denial', async () => {
  client.connect(); await flush();
  transport.operation = async () => JSON.stringify(failure('APPROVAL_REQUIRED', 'Approve again.', true));
  const result = await client.getHomeDirectory(); await flush();
  expect(result).toMatchObject({ ok: false, error: { code: 'APPROVAL_REQUIRED' } });
  expect(client.getState().status).toBe('awaiting-approval');
  transport.pair = async () => JSON.stringify(failure('ACCESS_DENIED', 'Revoked.', false));
  await vi.advanceTimersByTimeAsync(1000);
  expect(client.getState().status).toBe('rejected');
  await vi.advanceTimersByTimeAsync(60_000);
  expect(transport.calls('/v1/pair')).toHaveLength(2);
});

it('isolates observer exceptions and stops dispatch when an observer stops the client', async () => {
  client.subscribeState(() => { throw new Error('broken state observer'); });
  client.subscribe(() => { throw new Error('broken event observer'); });
  const event = vi.fn(() => client.stop()); client.subscribe(event);
  client.connect(); await flush();
  transport.events([
    { type: 'changed', event: { revision: 1, reason: 'sessions' } },
    { type: 'changed', event: { revision: 2, reason: 'sessions' } },
  ]); await flush();
  expect(event).toHaveBeenCalledTimes(1);
  expect(client.getState().status).toBe('disconnected');
});

it('rejects malformed event envelopes and caps response bytes before JSON decoding', async () => {
  client.connect(); await flush();
  transport.polls[0].resolve(JSON.stringify(success({ events: [{ type: 'unknown' }] }))); await flush();
  expect(client.getState().status).toBe('reconnecting');
  await vi.advanceTimersByTimeAsync(1000);
  transport.polls[1].resolve(' '.repeat(8 * 1024 * 1024 + 1)); await flush();
  expect(client.getState().status).toBe('reconnecting');
});

it('times out native requests without leaving JS polling active after stop', async () => {
  client.connect(); await flush();
  await vi.advanceTimersByTimeAsync(30_000);
  expect(client.getState().status).toBe('reconnecting');
  client.stop(); await vi.advanceTimersByTimeAsync(60_000);
  expect(transport.polls).toHaveLength(1);
  expect(vi.getTimerCount()).toBe(0);
});

it('validates peer identities, UUIDs and retry options', () => {
  expect(() => new RemoteClient('../bad', transport, { createClientId: () => ids.client })).toThrow();
  expect(() => new RemoteClient('host', transport, { createClientId: () => ids.client, approvalRetryMs: 0 })).toThrow();
  const invalid = new RemoteClient('host', transport, { createClientId: () => 'not-a-uuid' });
  expect(() => invalid.connect()).toThrow();
  expect(invalid.getState().status).toBe('disconnected');
});
