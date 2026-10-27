import { randomUUID } from 'node:crypto';
import { afterEach, expect, it, vi } from 'vitest';
import { RemoteClient } from '../../../mobile/src/client/client';
import type { TerminalEvent } from '../../shared/contracts';
import { remotePollResponseSchema } from '../../shared/remote-schemas';
import { RemoteHost } from './host';
import { embeddedFixture, identity, signedHeaders } from './test-fixtures';

const cleanups: Array<() => Promise<void>> = [];
afterEach(async () => { for (const cleanup of cleanups.splice(0).reverse()) await cleanup(); });
async function fixture() {
  const f = await embeddedFixture();
  cleanups.push(() => f.service.dispose());
  const created = await f.service.createSession({ cwd: '/work', requestId: randomUUID() });
  if (!created.ok) throw new Error(created.error.message);
  // The remote publisher may start after the agent has already become busy.
  f.processes[0].output('already working');
  const secret = 'activity-test-secret';
  const host = new RemoteHost(f.service, secret, () => true, () => {});
  const url = await host.listen();
  cleanups.push(() => host.dispose());
  const send = async (requestPath: string, payload: unknown) => {
    const body = JSON.stringify(payload);
    const response = await fetch(url + requestPath, { method: 'POST', body, headers: signedHeaders(secret, identity, requestPath, body) });
    return response.text();
  };
  const clientId = randomUUID(), tab = created.value.tabs[0];
  const pair = () => send('/v1/pair', { clientId });
  const poll = async () => {
    // Never wait 20 seconds for an empty poll, including on the unfixed host.
    f.service.selectSession(created.value.id);
    return remotePollResponseSchema.parse(JSON.parse(await send('/v1/events', { clientId })));
  };
  return { ...f, tab, send, clientId, pair, poll };
}
it('replays current activity on initial pair and same-view re-pair, including quiet and exit recovery', async () => {
  const f = await fixture();
  await f.pair();
  const busy = { type: 'terminal', event: { type: 'activity', tabId: f.tab.id, busy: true } };
  expect(await f.poll()).toMatchObject({ ok: true, value: { events: expect.arrayContaining([busy]) } });
  await f.pair();
  expect(await f.poll()).toMatchObject({ ok: true, value: { events: expect.arrayContaining([busy]) } });
  // The phone may discard this transition while suspended. A re-pair must recover it.
  await vi.waitFor(() => expect(f.backend.isBusy(f.tab.id)).toBe(false), { timeout: 4000 });
  await f.poll();
  await f.pair();
  expect(await f.poll()).toMatchObject({ ok: true, value: { events: expect.arrayContaining([
    { ...busy, event: { ...busy.event, busy: false } },
  ]) } });
  f.processes[0].exit(0); await f.poll(); await f.pair();
  expect(JSON.stringify(await f.poll())).not.toContain('"type":"activity"');
}, 10_000);
it('keeps recovery events for the new subscription when re-pairing retires an old blocked poll', async () => {
  const f = await fixture(); await f.pair(); await f.poll();
  // One poll blocks; the duplicate response confirms that it reached the host.
  const polls = [f.send('/v1/events', { clientId: f.clientId }), f.send('/v1/events', { clientId: f.clientId })];
  const duplicate = remotePollResponseSchema.parse(JSON.parse(await Promise.race(polls)));
  expect(duplicate).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  await f.pair();
  const retired = (await Promise.all(polls)).map(json => remotePollResponseSchema.parse(JSON.parse(json)));
  expect(retired).toContainEqual(expect.objectContaining({ ok: false, error: expect.objectContaining({ code: 'AUTH_FAILED', retryable: true }) }));
  // The old handler finished first, but must not have consumed the new epoch's seed.
  expect(await f.poll()).toMatchObject({ ok: true, value: { events: expect.arrayContaining([
    { type: 'terminal', event: { type: 'activity', tabId: f.tab.id, busy: true } },
  ]) } });
  // Its cleanup must also leave the next poll registered and wakeable.
  const next = [f.send('/v1/events', { clientId: f.clientId }), f.send('/v1/events', { clientId: f.clientId })];
  expect(remotePollResponseSchema.parse(JSON.parse(await Promise.race(next)))).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  f.processes[0].exit(0);
  const replies = (await Promise.all(next)).map(json => remotePollResponseSchema.parse(JSON.parse(json)));
  expect(replies).toContainEqual(expect.objectContaining({ ok: true, value: { events: expect.arrayContaining([
    { type: 'terminal', event: { type: 'activity', tabId: f.tab.id, busy: false } },
  ]) } }));
});
it('preserves queued activity when attaching or detaching while the output stays busy', async () => {
  const f = await fixture(); await f.pair();
  const terminal = { tabId: f.tab.id, generation: f.tab.generation };
  await f.send('/v1/request', { clientId: f.clientId, method: 'attachTerminal', payload: terminal });
  expect(await f.poll()).toMatchObject({ ok: true, value: { events: expect.arrayContaining([
    { type: 'terminal', event: { type: 'activity', tabId: f.tab.id, busy: true } },
  ]) } });
  await f.pair();
  await f.send('/v1/request', { clientId: f.clientId, method: 'detachTerminal', payload: terminal });
  expect(await f.poll()).toMatchObject({ ok: true, value: { events: expect.arrayContaining([
    { type: 'terminal', event: { type: 'activity', tabId: f.tab.id, busy: true } },
  ]) } });
});
it('coalesces activity snapshots per tab across repeated pairing without filling the event queue', async () => {
  const f = await fixture();
  for (let i = 1; i < 16; i++) {
    const added = await f.service.addTab({ sessionId: f.tab.sessionId });
    expect(added.ok).toBe(true);
  }
  for (let i = 0; i < 100; i++) await f.pair();
  const result = await f.poll();
  if (!result.ok) throw new Error(result.error.message);
  const activity = result.value.events.flatMap(event => event.type === 'terminal' && event.event.type === 'activity' ? [event.event] : []);
  expect(activity).toHaveLength(16);
  expect(new Set(activity.map(event => event.tabId)).size).toBe(16);
  expect(activity).toContainEqual({ type: 'activity', tabId: f.tab.id, busy: true });
  expect(activity.filter(event => !event.busy)).toHaveLength(15);
});
it('does not seed activity for elevated or stale terminal metadata', async () => {
  const f = await fixture(), tab = f.repository.tab(f.tab.id);
  if (!tab?.terminal) throw new Error('Missing terminal');
  const profiles = f.service.getTerminalProfiles();
  if (!profiles.ok) throw new Error('Missing profiles');
  vi.spyOn(f.service, 'getTerminalProfiles').mockReturnValue({ ok: true, value: {
    ...profiles.value, profiles: profiles.value.profiles.map(profile => ({ ...profile, elevation: 'administrator' })),
  } });
  await f.pair();
  expect(JSON.stringify(await f.poll())).not.toContain('"type":"activity"');
  vi.restoreAllMocks();
  f.repository.saveTab({ ...tab, operationId: randomUUID() });
  await f.pair();
  expect(JSON.stringify(await f.poll())).not.toContain('"type":"activity"');
});
it('restores the real mobile client activity subscription on reconnect without tab polling or output attachment', async () => {
  const f = await fixture(), events: TerminalEvent[] = [];
  const transport = { request: vi.fn(async (_peerId: string, requestPath: string, body: string) => f.send(requestPath, JSON.parse(body))) };
  const client = new RemoteClient('host', transport, { createClientId: randomUUID });
  cleanups.push(async () => { client.stop(); });
  client.subscribeTerminal(event => events.push(event));
  client.connect();
  const busy = { type: 'activity', tabId: f.tab.id, busy: true };
  await vi.waitFor(() => expect(events).toContainEqual(busy));
  events.length = 0; client.reconnect();
  await vi.waitFor(() => expect(events).toContainEqual(busy));
  events.length = 0; client.suspend();
  expect(client.getState().status).toBe('disconnected');
  // Drain the outstanding native long poll, just as an event during backgrounding would.
  f.service.selectSession(f.tab.sessionId); client.connect();
  await vi.waitFor(() => expect(events).toContainEqual(busy));
  expect(transport.request.mock.calls.some(([, requestPath]) => requestPath === '/v1/request')).toBe(false);
  // Subsequent ordinary transitions still arrive on the replacement subscription.
  f.processes[0].exit(0);
  await vi.waitFor(() => expect(events).toContainEqual({ ...busy, busy: false }));
});
