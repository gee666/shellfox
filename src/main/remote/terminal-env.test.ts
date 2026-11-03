import { afterEach, expect, it, vi } from 'vitest';
import { randomUUID } from 'node:crypto';
import { responseSchemas } from '../../shared/schemas';
import { remotePollResponseSchema } from '../../shared/remote-schemas';
import { RemoteHost } from './host';
import { embeddedFixture, identity, signedHeaders } from './test-fixtures';

const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => { for (const close of cleanups.splice(0).reverse()) await close(); });
async function fixture() {
  const f = await embeddedFixture();
  const secret = 'test-terminal-env-secret';
  let approved = false;
  const host = new RemoteHost(f.service, secret, () => approved, () => {});
  const url = await host.listen();
  cleanups.push(async () => { await host.dispose(); await f.service.dispose(); });
  const clientId = randomUUID();
  const send = async (requestPath: string, payload: unknown) => {
    const body = JSON.stringify(payload);
    const reply = await fetch(url + requestPath, { method: 'POST', body, headers: signedHeaders(secret, identity, requestPath, body) });
    return reply.json() as Promise<unknown>;
  };
  const request = (method: string, payload: unknown) => send('/v1/request', { clientId, method, payload });
  return { ...f, send, request, approve: () => { approved = true; }, pair: () => send('/v1/pair', { clientId }),
    poll: () => send('/v1/events', { clientId }), revoke: () => { approved = false; host.revoke(identity.id); } };
}

it('requires approval and pairing, restricts writes to env only, and keeps saveSettings forbidden', async () => {
  const f = await fixture();
  const env = [{ name: 'GLOBAL', value: 'literal $() ; 雪 = C:\\work' }, { name: 'EMPTY', value: '' }];
  const previous = f.repository.settings();
  expect(await f.request('setTerminalEnv', { env })).toMatchObject({ ok: false, error: { code: 'APPROVAL_REQUIRED' } });
  f.approve();
  expect(await f.request('setTerminalEnv', { env })).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  expect(f.repository.settings()).toEqual(previous);
  await f.pair();
  const result = responseSchemas.setTerminalEnv.parse(await f.request('setTerminalEnv', { env }));
  expect(result).toEqual({ ok: true, value: { ...previous, terminalEnv: env } });
  expect(f.repository.settings()).toEqual({ ...previous, terminalEnv: env });
  const events = remotePollResponseSchema.parse(await f.poll());
  expect(events).toMatchObject({ ok: true, value: { events: [{ type: 'changed', event: { reason: 'settings' } }] } });
  expect(await f.request('getSnapshot', {})).toMatchObject({ ok: true, value: { settings: { terminalEnv: env } } });
  expect(await f.request('saveSettings', { ...previous, terminalEnv: [] })).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  f.revoke();
  expect(await f.request('setTerminalEnv', { env: [] })).toMatchObject({ ok: false, error: { code: 'ACCESS_DENIED' } });
  expect(f.repository.settings().terminalEnv).toEqual(env);
});

it('does not persist a queued global environment write after its remote connection is revoked', async () => {
  const f = await fixture(); f.approve(); await f.pair();
  const previous = f.repository.settings();
  const save = vi.spyOn(f.repository, 'saveSettings');
  const changes = vi.fn();
  f.service.subscribe(changes);
  const setTerminalEnv = f.service.setTerminalEnv.bind(f.service);
  const queued = vi.spyOn(f.service, 'setTerminalEnv').mockImplementationOnce((input, canCommit) => {
    const result = setTerminalEnv(input, canCommit);
    // serialize() has queued the write, but its microtask has not run yet.
    expect(save).not.toHaveBeenCalled();
    f.revoke();
    return result;
  });
  expect(await f.request('setTerminalEnv', { env: [{ name: 'REVOKED', value: 'must not persist' }] }))
    .toMatchObject({ ok: false, error: { code: 'ACCESS_DENIED' } });
  expect(queued).toHaveBeenCalledOnce();
  expect(save).not.toHaveBeenCalled();
  expect(f.repository.settings()).toEqual(previous);
  expect(changes).not.toHaveBeenCalled();
  expect(await f.service.setTerminalEnv({ env: [{ name: 'LOCAL', value: 'still allowed' }] }))
    .toMatchObject({ ok: true, value: { terminalEnv: [{ name: 'LOCAL', value: 'still allowed' }] } });
});

it('rejects malformed and overbroad requests without calling the service', async () => {
  const f = await fixture(); f.approve(); await f.pair();
  const set = vi.spyOn(f.service, 'setTerminalEnv');
  const previous = f.repository.settings();
  for (const payload of [
    {}, { env: null }, { env: [], shellExecutable: '/bin/other' }, { env: [], processRules: [] },
    { env: [{ name: 'BAD=NAME', value: '' }] }, { env: [{ name: 'A', value: '\0' }] },
    { env: [{ name: 'A', value: 'line\nbreak' }] }, { env: [{ name: 'A', value: 'line\rbreak' }] },
    { env: [{ name: ' A ', value: 'one' }, { name: 'A', value: 'two' }] },
    { env: Array.from({ length: 201 }, (_, i) => ({ name: `V${i}`, value: '' })) },
  ]) expect(await f.request('setTerminalEnv', payload)).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  expect(set).not.toHaveBeenCalled();
  expect(f.repository.settings()).toEqual(previous);
});

it('applies host case validation and changes only future launches, preserving session overrides', async () => {
  const f = await fixture(); f.approve(); await f.pair();
  const created = responseSchemas.createSession.parse(await f.request('createSession', { cwd: '/work', requestId: randomUUID() }));
  if (!created.ok) throw new Error(created.error.message);
  const sessionId = created.value.id;
  const sessionEnv = [{ name: 'OVERRIDE', value: 'session' }];
  await f.request('setSessionEnv', { sessionId, env: sessionEnv });
  const env = [{ name: 'HOST_ONLY', value: 'host' }, { name: 'OVERRIDE', value: 'global' }];
  expect(await f.request('setTerminalEnv', { env })).toMatchObject({ ok: true });
  expect(f.processes).toHaveLength(1);
  expect(f.processes[0].kill).not.toHaveBeenCalled();
  expect(f.processes[0].write).not.toHaveBeenCalled();
  expect(f.repository.session(sessionId)?.env).toEqual(sessionEnv);
  expect(await f.request('addTab', { sessionId })).toMatchObject({ ok: true });
  expect(vi.mocked(f.factory).mock.calls[1][2].env).toMatchObject({ HOST_ONLY: 'host', OVERRIDE: 'session' });
  const distinct = [{ name: 'MODE', value: 'upper' }, { name: 'mode', value: 'lower' }];
  expect(await f.request('setTerminalEnv', { env: distinct })).toMatchObject(process.platform === 'win32'
    ? { ok: false, error: { code: 'VALIDATION' } } : { ok: true, value: { terminalEnv: distinct } });
  expect(await f.request('setTerminalEnv', { env: [] })).toMatchObject({ ok: true, value: { terminalEnv: [] } });
  expect(f.repository.session(sessionId)?.env).toEqual(sessionEnv);
});
