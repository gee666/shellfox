import { afterEach, expect, it, vi } from 'vitest';
import { z } from 'zod';
import { failure, success } from '../../shared/contracts';
import { RemoteView } from './view';

const cleanups: Array<() => Promise<void>> = [];
afterEach(async () => { for (const close of cleanups.splice(0)) await close(); });
const profiles = success({ profiles: [], defaultProfileId: null, lifetime: 'app-owned' as const, shellSurvival: false as const });
const requestIdentity = z.object({ clientId: z.uuid() });
function idle(signal?: AbortSignal): Promise<never> { return new Promise((_resolve, reject) => { signal?.addEventListener('abort', () => reject(new Error('closed')), { once: true }); }); }
it('preserves secondary-view approval and denial codes and never retries a denied attempt without an explicit reset', async () => {
  const attempts: string[] = [];
  const send = vi.fn(async (_peer: string, route: string, body: unknown, signal?: AbortSignal): Promise<unknown> => {
    if (route === '/v1/pair') {
      attempts.push(requestIdentity.parse(body).clientId);
      return attempts.length === 1 ? failure('APPROVAL_REQUIRED', 'pending', true) : attempts.length === 2 ? failure('ACCESS_DENIED', 'rejected') : success({ paired: true, terminalControl: 'exclusive-v1' });
    }
    if (route === '/v1/events') return idle(signal);
    return route === '/v1/request' ? profiles : success({ disconnected: true });
  });
  const denied = vi.fn();
  const view = new RemoteView(() => ({ status: 'connected', lastError: null }), send, () => {}, denied); cleanups.push(() => view.dispose());
  const input = { peerId: 'host', method: 'getTerminalProfiles', payload: {} };
  expect(await view.request(input)).toMatchObject({ ok: false, error: { code: 'APPROVAL_REQUIRED', retryable: true } });
  expect(await view.request(input)).toMatchObject({ ok: false, error: { code: 'ACCESS_DENIED', retryable: false } });
  expect(await view.request(input)).toMatchObject({ ok: false, error: { code: 'ACCESS_DENIED' } });
  expect(attempts).toHaveLength(2); expect(attempts[0]).toBe(attempts[1]);
  await view.disconnect('host'); expect(await view.request(input)).toEqual(profiles);
  expect(attempts[2]).not.toBe(attempts[0]); expect(denied).toHaveBeenCalledTimes(2);
});
it('keeps unsupported hosts stopped until reset and never classifies control contention as device denial', async () => {
  let supported = false, pairs = 0;
  const permission = vi.fn();
  const send = async (_peer: string, route: string, body: unknown, signal?: AbortSignal): Promise<unknown> => {
    if (route === '/v1/pair') { pairs++; return supported ? success({ paired: true, terminalControl: 'exclusive-v1' }) : success({ paired: true }); }
    if (route === '/v1/events') return idle(signal);
    if (route === '/v1/request' && body !== null && typeof body === 'object' && 'method' in body && body.method === 'writeTerminal') return failure('TERMINAL_CONTROL_REQUIRED', 'Take it first.');
    return route === '/v1/request' ? profiles : success({ disconnected: true });
  };
  const view = new RemoteView(() => ({ status: 'connected' }), send, () => {}, permission); cleanups.push(() => view.dispose());
  const input = { peerId: 'host', method: 'getTerminalProfiles', payload: {} };
  expect(await view.request(input)).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED', retryable: false } });
  expect(await view.request(input)).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } }); expect(pairs).toBe(1);
  supported = true; await view.disconnect('host'); expect(await view.request(input)).toEqual(profiles);
  expect(await view.request({ peerId: 'host', method: 'writeTerminal', payload: { tabId: '11111111-1111-4111-8111-111111111111', generation: '22222222-2222-4222-8222-222222222222', controlEpoch: 1, data: 'blocked' } })).toMatchObject({ ok: false, error: { code: 'TERMINAL_CONTROL_REQUIRED' } });
  expect(await view.request(input)).toEqual(profiles); expect(pairs).toBe(2); expect(permission).not.toHaveBeenCalled();
});
it('does not apply a stale denied response to a replacement secondary view', async () => {
  let finish: ((raw: unknown) => void) | undefined, pairs = 0;
  const stale = new Promise<unknown>(resolve => { finish = resolve; });
  const send = async (_peer: string, route: string, _body: unknown, signal?: AbortSignal): Promise<unknown> => {
    if (route === '/v1/pair') return ++pairs === 1 ? stale : success({ paired: true, terminalControl: 'exclusive-v1' });
    if (route === '/v1/events') return idle(signal);
    return route === '/v1/request' ? profiles : success({ disconnected: true });
  };
  const denied = vi.fn(), view = new RemoteView(() => ({ status: 'connected', lastError: null }), send, () => {}, denied); cleanups.push(() => view.dispose());
  const input = { peerId: 'host', method: 'getTerminalProfiles', payload: {} };
  const old = view.request(input); await view.disconnect('host'); expect(await view.request(input)).toEqual(profiles);
  finish?.(failure('ACCESS_DENIED', 'old attempt')); expect(await old).toMatchObject({ ok: false, error: { code: 'ACCESS_DENIED' } });
  expect(denied).not.toHaveBeenCalled(); expect(await view.request(input)).toEqual(profiles);
});
