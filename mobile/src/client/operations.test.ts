import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { failure, success, type TerminalEvent } from '../../../src/shared/contracts';
import type { RemoteHostEvent } from '../../../src/shared/remote-contracts';
import { RemoteClient } from './client';
import { attachment, deferred, flush, ids, session, TestTransport } from './test-fixtures';

let transport: TestTransport;
let client: RemoteClient;
beforeEach(async () => {
  vi.useFakeTimers(); transport = new TestTransport();
  client = new RemoteClient('host-1', transport, { createClientId: () => ids.client });
  client.connect(); await flush();
});
afterEach(() => { client.stop(); vi.useRealTimers(); });

it('validates payloads before native IO and checks method-specific responses', async () => {
  const before = transport.calls('/v1/request').length;
  expect(await client.createSession({ cwd: 'relative', requestId: ids.session })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(await client.resizeTerminal({ tabId: ids.tab, generation: ids.generation, cols: 1, rows: 24 })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(transport.calls('/v1/request')).toHaveLength(before);
  expect(await client.getHomeDirectory()).toEqual(success({ cwd: '/home/work' }));
  transport.operation = async () => JSON.stringify(success({ wrong: true }));
  expect(await client.getHomeDirectory()).toMatchObject({ ok: false, error: { code: 'NATIVE_UNAVAILABLE' } });
  await flush(); expect(client.getState().status).toBe('reconnecting');
});

it('forwards create/add/rename with shared payloads, preserves host errors, and never retries mutations', async () => {
  transport.operation = async () => JSON.stringify(success(session));
  expect(await client.createSession({ cwd: '/home/work', requestId: ids.session })).toEqual(success(session));
  expect(await client.addTab({ sessionId: ids.session, profileId: 'bash' })).toEqual(success(session));
  expect(await client.request('renameSession', { sessionId: ids.session, title: 'new name' })).toEqual(success(session));
  expect(transport.calls('/v1/request').map(body => JSON.parse(body).method)).toEqual(['createSession', 'addTab', 'renameSession']);
  transport.operation = async () => { throw new Error('lost response after host applied operation'); };
  expect(await client.addTab({ sessionId: ids.session })).toMatchObject({ ok: false, error: { code: 'NATIVE_UNAVAILABLE', retryable: true } });
  await vi.advanceTimersByTimeAsync(30_000);
  expect(transport.calls('/v1/request')).toHaveLength(4);
});

it('uses remote env validation without enforcing the mobile OS case policy', async () => {
  transport.operation = async () => JSON.stringify(success(session));
  const env = [{ name: 'path', value: '/a' }, { name: 'PATH', value: '/b' }];
  expect(await client.setSessionEnv({ sessionId: ids.session, env })).toEqual(success(session));
  expect(JSON.parse(transport.calls('/v1/request')[0]).payload).toEqual({ sessionId: ids.session, env });
});

it('enforces the encoded request byte limit, including JSON expansion', async () => {
  const env = Array.from({ length: 40 }, (_, i) => ({ name: `KEY_${i}`, value: 'x'.repeat(32767) }));
  const result = await client.setSessionEnv({ sessionId: ids.session, env });
  expect(result).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(transport.calls('/v1/request')).toHaveLength(0);
});

it('caps outstanding requests at eight, without counting the long poll', async () => {
  const pending = deferred<string>(); transport.operation = () => pending.promise;
  const requests = Array.from({ length: 8 }, () => client.getHomeDirectory()); await flush();
  expect(await client.getHomeDirectory()).toMatchObject({ ok: false, error: { code: 'VALIDATION', retryable: true } });
  expect(transport.calls('/v1/request')).toHaveLength(8);
  pending.resolve(JSON.stringify(success({ cwd: '/home/work' })));
  expect((await Promise.all(requests)).every(result => result.ok)).toBe(true);
});

it('discards stale in-flight results after stop', async () => {
  const pending = deferred<string>(); transport.operation = () => pending.promise;
  const operation = client.getHomeDirectory(); await flush(); client.stop();
  pending.resolve(JSON.stringify(success({ cwd: '/home/work' })));
  expect(await operation).toMatchObject({ ok: false, error: { code: 'NATIVE_UNAVAILABLE' } });
  expect(await client.getHomeDirectory()).toMatchObject({ ok: false, error: { code: 'NATIVE_UNAVAILABLE' } });
});

it('exposes a bound terminal API, forwards events, and only ACKs when the UI requests it', async () => {
  transport.operation = async body => {
    const { method } = JSON.parse(body);
    if (method === 'attachTerminal') return JSON.stringify(success(attachment));
    return JSON.stringify(success({ acknowledged: true }));
  };
  const { attachTerminal, acknowledgeTerminal } = client.terminalApi;
  const terminal = vi.fn<(event: TerminalEvent) => void>();
  const unsubscribe = client.subscribeTerminal(terminal);
  expect(await attachTerminal({ tabId: ids.tab })).toEqual(success(attachment));
  expect(transport.calls('/v1/request')).toHaveLength(1);
  expect(await acknowledgeTerminal({ tabId: ids.tab, generation: ids.generation, sequence: attachment.lastSequence })).toEqual(success({ acknowledged: true }));
  const event: TerminalEvent = { type: 'data', tabId: ids.tab, generation: ids.generation, sequence: 2, data: 'next' };
  transport.events([{ type: 'terminal', event }]); await flush();
  expect(terminal).toHaveBeenCalledWith(event);
  expect(transport.calls('/v1/request')).toHaveLength(2);
  unsubscribe(); transport.events([{ type: 'terminal', event }]); await flush();
  expect(terminal).toHaveBeenCalledTimes(1);
});

it('preserves control-lease denial and does not retry write, resize or close', async () => {
  const denied = failure('FOCUS_DENIED', 'Another remote client owns the 30 second lease.', true);
  transport.operation = async () => JSON.stringify(denied);
  const identity = { tabId: ids.tab, generation: ids.generation };
  expect(await client.terminalApi.writeTerminal({ ...identity, data: 'ls\r' })).toEqual(denied);
  expect(await client.terminalApi.resizeTerminal({ ...identity, cols: 120, rows: 40 })).toEqual(denied);
  expect(await client.closeTab(identity)).toEqual(denied);
  await flush();
  expect(client.getState().status).toBe('connected');
  expect(transport.calls('/v1/request')).toHaveLength(3);
});

it('does not overwrite a stop requested by a terminal-error observer', async () => {
  transport.operation = async () => JSON.stringify(success(attachment));
  await client.attachTerminal({ tabId: ids.tab });
  client.subscribeTerminal(event => { if (event.type === 'error') client.stop(); });
  transport.polls[0].reject(new Error('network lost')); await flush();
  expect(client.getState().status).toBe('disconnected');
  await vi.advanceTimersByTimeAsync(60_000);
  expect(transport.calls('/v1/pair')).toHaveLength(1);
});

it('invalidates attached views on reconnect, does not auto-reattach, and forgets detached views', async () => {
  transport.operation = async body => JSON.stringify(success(JSON.parse(body).method === 'attachTerminal' ? attachment : { detached: true }));
  const events: RemoteHostEvent[] = []; client.subscribe(event => events.push(event));
  await client.attachTerminal({ tabId: ids.tab });
  transport.polls[0].reject(new Error('network lost')); await flush();
  expect(events).toMatchObject([{ type: 'terminal', event: { type: 'error', tabId: ids.tab, generation: ids.generation } }]);
  await vi.advanceTimersByTimeAsync(1000);
  expect(transport.calls('/v1/request')).toHaveLength(1);
  await client.attachTerminal({ tabId: ids.tab });
  await client.detachTerminal({ tabId: ids.tab, generation: ids.generation });
  events.length = 0; client.stop();
  expect(events).toEqual([]);
});
