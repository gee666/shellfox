import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { RustHostClient } from './rust-host-client';
import type { RemoteClientOptions } from '../client/host-client';
import { nativeTerminalForApi } from './native-terminal';
import { ids, deferred } from '../client/test-fixtures';
import { TestNative } from './test-native';
import { keyInput } from './key-input';
import { emptyModifiers } from '../terminal/protocol';
import parity from '../../../crates/shellfox-client/tests/fixtures/remote-parity.json';
import { z } from 'zod';
import { remoteMethods, remotePayloadSchemas } from '../../../src/shared/remote-schemas';
import { rustRemoteMethods } from './remote-adapter';
import { parseResponse } from '../client/protocol';

const clients: RustHostClient[] = [];
const config = { foreground: '#eeeeee', background: '#111111', cursor: '#ffcc00' };
function setup(options: Partial<RemoteClientOptions> = {}) {
  const native = new TestNative();
  const client = new RustHostClient('node_123', native, { createClientId: () => ids.client, ...options });
  clients.push(client); return { native, client };
}
async function tick(ms = 120) { await vi.advanceTimersByTimeAsync(ms); }
beforeEach(() => vi.useFakeTimers());
afterEach(async () => {
  for (const client of clients.splice(0)) client.stop();
  await tick(300); vi.useRealTimers();
});

describe('Rust host client in existing mobile presentation', () => {
  it('is inert until the existing connection lifecycle explicitly starts it', async () => {
    const { native, client } = setup();
    expect(client.getState().status).toBe('disconnected');
    expect(native.createClient).not.toHaveBeenCalled();
    const state = vi.fn(); client.subscribeState(state);
    client.connect(); await tick();
    expect(native.createClient).toHaveBeenCalledWith('node_123', ids.client);
    expect(client.getState()).toMatchObject({ status: 'connected', connectionId: '1:1' });
    expect(native.poll).toHaveBeenCalledTimes(1);
    const read = client.getHomeDirectory(); await tick();
    expect(await read).toEqual({ ok: true, value: { cwd: '/home/work' } });
    expect(native.operation).toHaveBeenCalledWith('1', '{"method":"getHomeDirectory","payload":{}}');
    const notifications = state.mock.calls.length;
    native.complete({ ok: true, value: { acknowledged: true } }); await tick();
    expect(state).toHaveBeenCalledTimes(notifications);
  });

  it('resolves explicit native-internal polls and delivers unsolicited coarse host events exactly once', async () => {
    const { native, client } = setup(); const listener = vi.fn(); client.subscribe(listener);
    client.connect(); await tick();
    const poll = native.polls[0];
    native.event(poll, { ok: true, value: { events: [{ type: 'changed', event: { revision: 1, reason: 'settings' } }] } }, 0, true);
    await tick();
    expect(native.poll).toHaveBeenCalledTimes(2);
    expect(listener).toHaveBeenCalledTimes(1);
    native.event('500', { ok: true, value: { events: [{ type: 'changed', event: { revision: 2, reason: 'settings' } }] } }, 0, true);
    native.event('501', { ok: true, value: { acknowledged: true } }, 0, true);
    await tick();
    expect(listener).toHaveBeenCalledTimes(2);
    expect(listener).toHaveBeenLastCalledWith({ type: 'changed', event: { revision: 2, reason: 'settings' } });
  });

  it('keeps the complete remote allowlist and all 16 ported file/directory/upload contracts', async () => {
    expect(new Set([...rustRemoteMethods, 'writeTerminal', 'acknowledgeTerminal'])).toEqual(new Set(remoteMethods));
    const fixtures = z.array(z.object({ method: z.enum(rustRemoteMethods), payload: z.unknown(), response: z.unknown() }).strict()).parse(parity);
    expect(fixtures).toHaveLength(16);
    const { native, client } = setup(); client.connect(); await tick();
    for (const fixture of fixtures) {
      native.result = () => fixture.response;
      const payload = remotePayloadSchemas[fixture.method].parse(fixture.payload);
      const operation = client.request(fixture.method, payload); await tick();
      expect(await operation).toEqual(parseResponse(fixture.method, fixture.response));
    }
    expect(native.operation).toHaveBeenCalledTimes(16);
  });

  it('validates the real request/response contract and rejects local or raw terminal paths', async () => {
    const { native, client } = setup(); client.connect(); await tick();
    expect((await client.request('createSession', { cwd: 'http://localhost:123', requestId: ids.client })).ok).toBe(false);
    expect(native.operation).not.toHaveBeenCalled();
    const raw = await client.writeTerminal({ tabId: ids.tab, generation: ids.generation, controlEpoch: 1, data: 'not an encoded native input' });
    expect(raw).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
    const legacy = await client.attachTerminal({ tabId: ids.tab });
    expect(legacy).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
    native.result = () => ({ ok: true, value: { cwd: '/home/work', secret: 'bad' } });
    const read = client.getHomeDirectory(); await tick();
    expect(await read).toMatchObject({ ok: false, error: { code: 'NATIVE_UNAVAILABLE' } });
  });

  it('binds and attaches native handles without exposing output to JavaScript', async () => {
    const { native, client } = setup(); client.connect(); await tick();
    const driver = nativeTerminalForApi(client.terminalApi);
    expect(driver).toBeDefined();
    if (!driver) throw new Error('Missing native terminal registration.');
    const opening = driver.open(ids.tab, false, config); await tick();
    const view = await opening;
    expect(native.bindTerminal).toHaveBeenCalledWith('1', ids.tab, view.handle);
    expect(view.getState()).toBe('attached');
    expect(view.getIdentity()).toBeNull();
    await view.input({ type: 'paste', text: 'not ready' });
    expect(native.terminalInput).not.toHaveBeenCalled();
    view.nativeState('ready');
    expect(view.getIdentity()).toEqual({ tabId: ids.tab, generation: ids.generation, controlEpoch: 1 });
    expect(native.configureTerminal).toHaveBeenCalledWith(view.handle, JSON.stringify(config));
    await view.input(keyInput('ArrowUp', { ...emptyModifiers, ctrl: true }));
    expect(native.terminalInput).toHaveBeenCalledWith(view.handle, JSON.stringify({
      type: 'key', key: { kind: 'arrowUp' }, action: 'press', modifiers: 2,
    }));
    expect(native.operation.mock.calls.every(([, json]) => !json.includes('writeTerminal'))).toBe(true);
    view.nativeState('recovering');
    expect(view.getIdentity()).toBeNull();
    await view.input({ type: 'paste', text: 'still recovering' });
    expect(native.terminalInput).toHaveBeenCalledTimes(1);
    expect(native.events.every((event) => !JSON.stringify(event).includes('hello'))).toBe(true);
    const closing = view.close(); await tick(); await closing;
    expect(native.unbindTerminal).toHaveBeenCalledWith('1', view.handle);
    expect(native.closeTerminal).toHaveBeenCalledWith(view.handle);
  });

  it('fences a pending native creation and rejects duplicate views without leaking its handle', async () => {
    const { native, client } = setup(); client.connect(); await tick();
    const driver = nativeTerminalForApi(client.terminalApi);
    if (!driver) throw new Error('Missing native terminal registration.');
    const creation = deferred<string>();
    native.createTerminal.mockImplementationOnce(() => creation.promise);
    const opening = driver.open(ids.tab, false, config);
    const rejected = expect(opening).rejects.toThrow('connection changed');
    await expect(driver.open(ids.tab, false, config)).rejects.toThrow('already has');
    await driver.close();
    creation.resolve('150'); await tick(); await rejected;
    expect(native.closeTerminal).toHaveBeenCalledWith('150');
    expect(native.bindTerminal).not.toHaveBeenCalled();
  });

  it('keeps automation selection possible but suppresses native control and input', async () => {
    const { native, client } = setup(); client.connect(); await tick();
    const driver = nativeTerminalForApi(client.terminalApi);
    if (!driver) throw new Error('Missing native terminal registration.');
    const opening = driver.open(ids.tab, true, config); await tick();
    const view = await opening;
    view.nativeState('ready');
    await view.input({ type: 'paste', text: 'unsafe write' });
    await view.takeControl();
    expect(native.terminalInput).not.toHaveBeenCalled();
    expect(native.operation.mock.calls.some(([, json]) => json.includes('takeTerminalControl'))).toBe(false);
  });

  it('treats native host revocation as a denial and never automatically pairs again', async () => {
    const { native, client } = setup(); client.connect(); await tick();
    native.connection = { status: 'rejected', permission: 'rejected', connectionId: '1', retryAfterMs: null };
    native.event('700', { ok: false, error: { code: 'ACCESS_DENIED', message: 'Revoked.', retryable: false } }, 0, true);
    await tick(5000);
    expect(client.getState()).toMatchObject({ status: 'rejected', lastError: { code: 'ACCESS_DENIED' } });
    expect(native.connect).toHaveBeenCalledTimes(1);
    expect(native.reconnect).not.toHaveBeenCalled();
    expect(native.closeClient).toHaveBeenCalledOnce();
  });

  it('suspends an in-flight pair without withdrawal, retries or background polling', async () => {
    const { native, client } = setup(); native.connect.mockResolvedValueOnce('900');
    client.connect(); await tick(); client.suspend();
    for (let i = 0; i < 20; i++) { client.connect(); client.suspend(); }
    native.connection = { status: 'awaitingApproval', permission: 'awaitingApproval', connectionId: '0', retryAfterMs: 1000 };
    native.event('900', { ok: false, error: { code: 'APPROVAL_REQUIRED', message: 'Approval pending.', retryable: true } });
    await tick(5000);
    expect(client.getState().status).toBe('awaiting-approval');
    expect(native.closeClient).not.toHaveBeenCalled(); expect(native.connect).toHaveBeenCalledOnce();
    expect(native.reconnect).not.toHaveBeenCalled(); expect(native.poll).not.toHaveBeenCalled();
    expect(native.operation).not.toHaveBeenCalled();
    native.connection = { status: 'connected', permission: 'approved', connectionId: '1', retryAfterMs: null };
    client.connect(); await tick();
    expect(native.createClient).toHaveBeenCalledOnce(); expect(native.connect).toHaveBeenCalledTimes(2);
    expect(native.reconnect).not.toHaveBeenCalled(); expect(client.getState().status).toBe('connected');
  });

  it('retains the same native pairing attempt for foreground approval retries', async () => {
    const { native, client } = setup();
    native.connect.mockImplementation(async handle => {
      native.connection = { status: 'awaitingApproval', permission: 'awaitingApproval', connectionId: '0', retryAfterMs: 1000 };
      return native.complete({ ok: false, error: { code: 'APPROVAL_REQUIRED', message: 'Pending.', retryable: true } }, 0, handle);
    });
    client.connect(); await tick(2500);
    expect(native.connect.mock.calls.length).toBeGreaterThan(1);
    expect(native.connect.mock.calls.every(([handle]) => handle === '1')).toBe(true);
    expect(native.createClient).toHaveBeenCalledOnce(); expect(native.reconnect).not.toHaveBeenCalled();
    client.suspend(); const count = native.connect.mock.calls.length; await tick(5000);
    expect(native.connect).toHaveBeenCalledTimes(count); expect(native.closeClient).not.toHaveBeenCalled();
  });

  it('retains a successful in-flight pair but defers connected state/polling until resume', async () => {
    const { native, client } = setup(); native.connect.mockResolvedValueOnce('900');
    client.connect(); await tick(); client.suspend();
    native.event('900', { ok: true, value: { paired: true, terminalControl: 'exclusive-v1' } });
    await tick(5000);
    expect(client.getState().status).not.toBe('connected');
    expect(native.poll).not.toHaveBeenCalled(); expect(native.closeClient).not.toHaveBeenCalled();
    expect((await client.getHomeDirectory()).ok).toBe(false); expect(native.operation).not.toHaveBeenCalled();
    client.connect(); await tick();
    expect(client.getState().status).toBe('connected'); expect(native.connect).toHaveBeenCalledOnce();
    expect(native.poll).toHaveBeenCalledOnce();
  });

  it('does not let repeated foreground discovery connect calls bypass retry backoff', async () => {
    const { native, client } = setup({ reconnectDelayMs: 5000, maxReconnectDelayMs: 5000 });
    native.connect.mockImplementation(async handle => {
      native.connection = { status: 'reconnecting', permission: 'unknown', connectionId: '0', retryAfterMs: 5000 };
      return native.complete({ ok: false, error: { code: 'NATIVE_UNAVAILABLE', message: 'Unavailable.', retryable: true } }, 0, handle);
    });
    client.connect(); await tick();
    for (let i = 0; i < 4; i++) { client.connect(); await tick(500); }
    expect(native.connect).toHaveBeenCalledOnce();
    await tick(3500); expect(native.connect).toHaveBeenCalledTimes(2);
    expect(native.reconnect).not.toHaveBeenCalled(); expect(native.createClient).toHaveBeenCalledOnce();
  });

  it('records rejection while suspended and requires an explicit new request', async () => {
    const { native, client } = setup(); native.connect.mockResolvedValueOnce('900');
    client.connect(); await tick(); client.suspend();
    native.connection = { status: 'rejected', permission: 'rejected', connectionId: '0', retryAfterMs: null };
    native.event('900', { ok: false, error: { code: 'ACCESS_DENIED', message: 'Denied.', retryable: false } });
    await tick(5000);
    expect(client.getState().status).toBe('rejected'); expect(native.connect).toHaveBeenCalledOnce();
    expect(native.closeClient).toHaveBeenCalledOnce(); expect(native.poll).not.toHaveBeenCalled();
  });

  it('re-pairs a suspended connection with a new epoch and never replays retired input', async () => {
    const { native, client } = setup(); client.connect(); await tick();
    const driver = nativeTerminalForApi(client.terminalApi); if (!driver) throw new Error('Missing native driver.');
    const opening = driver.open(ids.tab, false, config); await tick(); const view = await opening;
    view.nativeState('ready'); await view.input({ type: 'paste', text: 'before' });
    const previous = client.getState().connectionId;
    client.suspend(); await tick();
    expect(client.getState().status).toBe('reconnecting'); expect(native.closeClient).not.toHaveBeenCalled();
    expect(native.closeTerminal).toHaveBeenCalledWith(view.handle);
    native.event(native.polls[0], { ok: true, value: { events: [] } }); await tick(5000);
    expect(native.poll).toHaveBeenCalledOnce(); expect(native.reconnect).not.toHaveBeenCalled();
    client.connect(); await tick();
    expect(native.createClient).toHaveBeenCalledOnce(); expect(native.reconnect).toHaveBeenCalledOnce();
    expect(client.getState().connectionId).not.toBe(previous);
    await view.input({ type: 'paste', text: 'retired' });
    const nextOpening = driver.open(ids.tab, false, config); await tick(); const next = await nextOpening;
    next.nativeState('ready'); expect(native.terminalInput).toHaveBeenCalledOnce();
    await next.input({ type: 'paste', text: 'after' }); expect(native.terminalInput).toHaveBeenCalledTimes(2);
    expect(native.operation.mock.calls.some(([, json]) => json.includes('takeTerminalControl'))).toBe(false);
  });

  it('ignores an old request completion after explicit stop and a new native client', async () => {
    const { native, client } = setup(); native.connect.mockResolvedValueOnce('900');
    client.connect(); await tick(); client.stop(); await tick(); client.connect(); await tick();
    expect(client.getState()).toMatchObject({ status: 'connected', connectionId: '2:1' });
    native.event('900', { ok: false, error: { code: 'ACCESS_DENIED', message: 'Old request.', retryable: false } }, 0, false, '1');
    await tick(); expect(client.getState()).toMatchObject({ status: 'connected', connectionId: '2:1' });
    expect(native.createClient).toHaveBeenCalledTimes(2);
  });

  it('surfaces unsupported host terminal recovery instead of selecting a WebView fallback', async () => {
    const { native, client } = setup(); client.connect(); await tick();
    native.result = () => ({ ok: false, error: { code: 'UNSUPPORTED', message: 'Full-state terminal recovery is unavailable.', retryable: false } });
    const driver = nativeTerminalForApi(client.terminalApi);
    if (!driver) throw new Error('Missing native terminal registration.');
    const opening = driver.open(ids.tab, false, config);
    const rejected = expect(opening).rejects.toThrow('Full-state');
    await tick(250); await rejected;
    expect(nativeTerminalForApi(client.terminalApi)).toBe(driver);
    expect(native.closeTerminal).toHaveBeenCalled();
  });
});
