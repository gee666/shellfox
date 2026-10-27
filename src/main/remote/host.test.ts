import { afterEach, expect, it, vi } from 'vitest';
import { randomUUID } from 'node:crypto';
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import path from 'node:path';
import { startHostRpc } from '../../host/rpc';
import { connectHostClient } from '../../host/client';
import { RemoteHost } from './host';
import { embeddedFixture, identity, signedHeaders } from './test-fixtures';
import { responseSchemas } from '../../shared/schemas';
import { remotePollResponseSchema } from '../../shared/remote-schemas';
const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => { for (const cleanup of cleanups.splice(0).reverse()) await cleanup(); });
async function fixture() {
  const f = await embeddedFixture(), secret = 'test-secret'; let approved = false;
  const pending = vi.fn(), host = new RemoteHost(f.service, secret, () => approved, pending);
  const url = await host.listen(); cleanups.push(async () => { await host.dispose(); await f.service.dispose(); });
  const send = async (requestPath: string, payload: unknown, peer = identity, override?: Record<string, string>) => {
    const body = JSON.stringify(payload);
    const response = await fetch(url + requestPath, { method: 'POST', body, headers: { ...signedHeaders(secret, peer, requestPath, body), ...override } });
    return response.json() as Promise<unknown>;
  };
  const clientId = randomUUID();
  const request = (method: string, payload: unknown, client = clientId) => send('/v1/request', { clientId: client, method, payload });
  return { ...f, host, send, clientId, request, pending, approve: () => { approved = true; }, revoke: () => { approved = false; host.revoke(identity.id); } };
}
it('requires signed WhoIs identity and explicit pairing before metadata or terminals', async () => {
  const f = await fixture();
  expect(await f.send('/v1/pair', { clientId: f.clientId }, identity, { 'x-shellfox-signature': '0'.repeat(64) })).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  expect(f.pending).not.toHaveBeenCalled();
  expect(await f.request('getSnapshot', {})).toMatchObject({ ok: false, error: { code: 'APPROVAL_REQUIRED', retryable: true } });
  expect(f.pending).toHaveBeenCalledWith(identity, f.clientId); f.approve();
  expect(await f.request('getSnapshot', {})).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  expect(await f.send('/v1/pair', { clientId: f.clientId })).toEqual({ ok: true, value: { paired: true, terminalControl: 'exclusive-v1' } });
  expect(await f.request('getSnapshot', {})).toMatchObject({ ok: true });
  for (const method of ['saveSettings', 'listSshProfiles', 'setExplorerIntegration', 'openExternal', 'retryTab']) expect(await f.request(method, {})).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  f.revoke(); expect(await f.request('getSnapshot', {})).toMatchObject({ ok: false, error: { code: 'ACCESS_DENIED', retryable: false } });
});
it('rechecks authorization after an in-flight stats read before returning metadata', async () => {
  const f = await fixture(); f.approve(); await f.send('/v1/pair', { clientId: f.clientId });
  let finish: (() => void) | undefined;
  const pending = new Promise<void>(resolve => { finish = resolve; });
  const stats = vi.spyOn(f.service, 'getSessionStats').mockImplementation(async () => { await pending; return { ok: true, value: { sessions: [], tabs: [] } }; });
  const reply = f.request('getSessionStats', {}); await vi.waitFor(() => expect(stats).toHaveBeenCalledOnce());
  f.revoke(); finish?.();
  expect(await reply).toMatchObject({ ok: false, error: { code: 'ACCESS_DENIED', retryable: false } });
});
it('delivers only to the owner and automatically promotes a waiting remote view after disconnect', async () => {
  const f = await fixture(); f.approve(); await f.send('/v1/pair', { clientId: f.clientId });
  const created = responseSchemas.createSession.parse(await f.request('createSession', { cwd: '/work', requestId: randomUUID() }));
  if (!created.ok) throw new Error('create failed');
  const tab = created.value.tabs[0], terminal = { tabId: tab.id, generation: tab.generation! };
  f.processes[0].output('before attach');
  const attached = responseSchemas.attachTerminal.parse(await f.request('attachTerminal', terminal));
  if (!attached.ok || attached.value.control !== 'owned') throw new Error('No owner');
  const controlled = { ...terminal, controlEpoch: attached.value.controlEpoch };
  expect(attached.value.chunks.map(c => c.data).join('')).toContain('before attach');
  await f.request('acknowledgeTerminal', { ...controlled, sequence: attached.value.lastSequence });
  f.processes[0].output('live');
  const poll = remotePollResponseSchema.parse(await f.send('/v1/events', { clientId: f.clientId }));
  expect(poll).toMatchObject({ ok: true, value: { events: expect.arrayContaining([expect.objectContaining({ type: 'terminal', event: expect.objectContaining({ type: 'data', data: 'live', controlEpoch: attached.value.controlEpoch }) })]) } });
  expect(await f.request('resizeTerminal', { ...controlled, cols: 100, rows: 35 })).toEqual({ ok: true, value: { resized: true } });
  const second = randomUUID(); await f.send('/v1/pair', { clientId: second });
  expect(await f.request('attachTerminal', terminal, second)).toMatchObject({ ok: true, value: { control: 'blocked' } });
  expect(await f.request('writeTerminal', { ...controlled, data: 'other' }, second)).toMatchObject({ ok: false, error: { code: 'TERMINAL_CONTROL_REQUIRED' } });
  f.processes[0].output('queued secret before takeover');
  expect(await f.request('takeTerminalControl', terminal, second)).toEqual({ ok: true, value: { taken: true } });
  const revoked = remotePollResponseSchema.parse(await f.send('/v1/events', { clientId: f.clientId }));
  expect(JSON.stringify(revoked)).not.toContain('queued secret before takeover');
  expect(revoked).toMatchObject({ ok: true, value: { events: expect.arrayContaining([expect.objectContaining({ type: 'terminal', event: expect.objectContaining({ type: 'control', control: 'blocked' }) })]) } });
  await f.request('takeTerminalControl', terminal); await f.request('attachTerminal', terminal);
  await f.send('/v1/disconnect', { clientId: f.clientId });
  expect(f.processes[0].kill).not.toHaveBeenCalled();
  const promoted = responseSchemas.attachTerminal.parse(await f.request('attachTerminal', terminal, second));
  if (!promoted.ok || promoted.value.control !== 'owned') throw new Error('No promoted owner');
  expect(await f.request('writeTerminal', { ...terminal, controlEpoch: promoted.value.controlEpoch, data: 'new controller' }, second)).toEqual({ ok: true, value: { written: true } });
  f.revoke(); expect(f.processes[0].kill).not.toHaveBeenCalled(); expect(f.service.ownedTerminalCount()).toBe(1);
});
it('keeps explicit authenticated session deletion separate from terminal takeover', async () => {
  const f = await fixture(); f.approve(); await f.send('/v1/pair', { clientId: f.clientId });
  const session = await f.service.createSession({ cwd: '/work', requestId: randomUUID() });
  if (!session.ok) throw new Error('No session');
  await f.request('attachTerminal', { tabId: session.value.tabs[0].id });
  const other = randomUUID(); await f.send('/v1/pair', { clientId: other });
  expect(await f.request('deleteSession', { sessionId: session.value.id }, other)).toEqual({ ok: true, value: { deleted: true } });
  expect(f.processes[0].kill).toHaveBeenCalledOnce();
});
it('arbitrates one owner across remote HTTP, local host RPC and a host-local desktop view', async () => {
  const f = await fixture(); f.approve(); await f.send('/v1/pair', { clientId: f.clientId });
  const session = await f.service.createSession({ cwd: '/work', requestId: randomUUID() });
  if (!session.ok) throw new Error('No session');
  const id = { tabId: session.value.tabs[0].id, generation: session.value.tabs[0].generation! };
  const remote = responseSchemas.attachTerminal.parse(await f.request('attachTerminal', id));
  if (!remote.ok || remote.value.control !== 'owned') throw new Error('No remote owner');
  await mkdir('tmp', { recursive: true }); const directory = await mkdtemp(path.resolve('tmp/exclusive-transports-'));
  const rpc = await startHostRpc({ userData: directory, service: f.service, mode: 'headless', shutdown: async () => {} });
  const client = await connectHostClient(directory), localEvents: unknown[] = [];
  const local = f.service.createTerminalView(event => localEvents.push(event));
  cleanups.push(async () => { local.dispose(); await client.close(); await rpc.close(); await rm(directory, { recursive: true, force: true }); });
  expect(await client.service.attachTerminal(id)).toMatchObject({ ok: true, value: { control: 'blocked' } });
  expect(local.attach(id)).toMatchObject({ ok: true, value: { control: 'blocked' } });
  f.processes[0].output('remote queued content'); local.take(id);
  const lost = remotePollResponseSchema.parse(await f.send('/v1/events', { clientId: f.clientId }));
  expect(JSON.stringify(lost)).not.toContain('remote queued content');
  expect(await f.request('writeTerminal', { ...id, controlEpoch: remote.value.controlEpoch, data: 'late remote' })).toMatchObject({ ok: false, error: { code: 'TERMINAL_CONTROL_REQUIRED' } });
  await client.service.takeTerminalControl(id);
  const owned = await client.service.attachTerminal(id);
  if (!owned.ok || owned.value.control !== 'owned') throw new Error('No RPC owner');
  expect(localEvents).toContainEqual(expect.objectContaining({ type: 'control', control: 'blocked' }));
  expect(await client.service.writeTerminal({ ...id, controlEpoch: owned.value.controlEpoch, data: 'RPC owner' })).toMatchObject({ ok: true });
  await client.close();
  expect(await f.request('attachTerminal', id)).toMatchObject({ ok: true, value: { control: 'owned' } });
  expect(f.processes[0].kill).not.toHaveBeenCalled();
});
it('routes per-client visible sessions and clears interests when views disconnect', async () => {
  const f = await fixture(); f.approve(); await f.send('/v1/pair', { clientId: f.clientId });
  const set = vi.spyOn(f.service, 'setRemoteVisibleSessions'), clear = vi.spyOn(f.service, 'clearRemoteVisibleSessions');
  const sessionId = randomUUID();
  expect(await f.request('setVisibleSessions', { sessionIds: [sessionId] })).toEqual({ ok: true, value: { configured: true } });
  expect(set).toHaveBeenCalledWith(identity.id + '/' + f.clientId, { sessionIds: [sessionId] });
  await f.send('/v1/disconnect', { clientId: f.clientId });
  expect(clear).toHaveBeenCalledWith(identity.id + '/' + f.clientId);
});
it('rejects nonexistent and stale terminal identities before PTY access', async () => {
  const f = await fixture(); f.approve(); await f.send('/v1/pair', { clientId: f.clientId });
  const created = await f.service.createSession({ cwd: '/work', requestId: randomUUID() }); if (!created.ok) throw new Error('create failed');
  for (const tabId of [randomUUID(), created.value.tabs[0].id]) expect(await f.request('writeTerminal', { tabId, generation: randomUUID(), controlEpoch: 1, data: 'no' })).toMatchObject({ ok: false, error: { code: 'TARGET_LOST' } });
  expect(f.processes[0].write).not.toHaveBeenCalled();
});
it('keeps elevated terminals local-only for direct and session-wide operations and hides metadata', async () => {
  const f = await fixture(); f.approve(); await f.send('/v1/pair', { clientId: f.clientId });
  const created = await f.service.createSession({ cwd: '/work', requestId: randomUUID() }); if (!created.ok) throw new Error('create failed');
  const tab = f.repository.tabs(created.value.id)[0]; if (!tab.terminal) throw new Error('terminal missing');
  tab.terminal.profileId = 'administrator-powershell'; f.repository.saveTab(tab);
  const profiles = f.service.getTerminalProfiles(); if (!profiles.ok) throw new Error('profiles missing');
  vi.spyOn(f.service, 'getTerminalProfiles').mockReturnValue({ ok: true, value: { ...profiles.value, profiles: [...profiles.value.profiles, { ...profiles.value.profiles[0], id: 'administrator-powershell', elevation: 'administrator' }] } });
  for (const [method, payload] of [
    ['attachTerminal', { tabId: tab.id }], ['resizeTerminal', { tabId: tab.id, generation: tab.operationId, controlEpoch: 1, cols: 80, rows: 24 }],
    ['writeTerminal', { tabId: tab.id, generation: tab.operationId, controlEpoch: 1, data: 'no' }], ['closeTab', { tabId: tab.id, generation: tab.operationId }],
    ['deleteSession', { sessionId: tab.sessionId }], ['addTab', { sessionId: tab.sessionId, profileId: 'administrator-powershell' }],
  ] as const) expect(await f.request(method, payload)).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  expect(await f.request('renameSession', { sessionId: tab.sessionId, title: 'renamed' })).toMatchObject({ ok: true, value: { tabs: [], counts: { running: 0, waiting: 0, unknown: 0, error: 0, closed: 0, agents: 0 } } });
  expect(f.processes[0].kill).not.toHaveBeenCalled();
});
it('coalesces metadata without accumulating phantom queue bytes', async () => {
  const f = await fixture(); f.approve(); await f.send('/v1/pair', { clientId: f.clientId });
  const created = await f.service.createSession({ cwd: '/work', requestId: randomUUID() }); if (!created.ok) throw new Error('create failed');
  for (let i = 0; i < 30_000; i++) f.service.selectSession(created.value.id);
  const result = remotePollResponseSchema.parse(await f.send('/v1/events', { clientId: f.clientId }));
  expect(result).toMatchObject({ ok: true, value: { events: [{ type: 'changed' }] } });
  if (!result.ok) throw new Error('poll failed');
  const changed = result.value.events[0];
  expect(changed.type).toBe('changed');
  if (changed.type === 'changed') expect(changed.event).toEqual({ revision: f.service.revision, reason: 'sessions' });
  // The host CLI selected this session 30,000 times, but remote views only
  // receive metadata changes, never a command to change their input target.
  expect(JSON.stringify(result)).not.toContain('selectSessionId');
});
