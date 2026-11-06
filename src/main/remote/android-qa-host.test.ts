import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { mkdir, mkdtemp, rm, writeFile, readFile, symlink, readdir } from 'node:fs/promises';
import { afterEach, expect, it, vi } from 'vitest';
import { RemoteHost } from './host';
import { RemoteApprovals } from './approvals';
import { embeddedFixture, signedHeaders } from './test-fixtures';
import { startQaRemoteRelay, type QaRequest } from '../../../mobile/e2e/remote-host-relay';

const cleanups: Array<() => Promise<void>> = [];
afterEach(async () => { for (const close of cleanups.splice(0).reverse()) await close(); });
async function fixture() {
  await mkdir('tmp', { recursive: true }); const directory = await mkdtemp(path.resolve('tmp/android-real-host-test-')); cleanups.push(() => rm(directory, { recursive: true, force: true }));
  const root = path.join(directory, 'public'), outside = path.join(directory, 'outside'); await mkdir(root); await mkdir(outside);
  await writeFile(path.join(root, 'code.ts'), 'actual Windows test disk'); await writeFile(path.join(outside, 'secret.txt'), 'must not leak');
  const f = await embeddedFixture(); cleanups.push(() => f.service.dispose());
  const peer = { id: 'android-real-files-qa', hostname: 'Android emulator QA' }, secret = 'synthetic-test-secret', controlToken = 'host-runner-control-only', approved = new Set<string>(), audit: QaRequest[] = [];
  const approvals = new RemoteApprovals(id => approved.has(id), () => {});
  const host = new RemoteHost(f.service, secret, id => approved.has(id), (p, id) => { approvals.authorize(p, id); }, (p, id) => approvals.authorize(p, id), (p, id) => approvals.withdraw(p.id, id), { root: path.join(directory, 'unused-inbox') });
  const backendUrl = await host.listen(); cleanups.push(() => host.dispose());
  const relay = await startQaRemoteRelay({ port: 0, backendUrl, configuration: { protocol: 1, hostId: 'windows-real-host-qa', clientPeer: peer, hmacSecret: secret, root, files: ['code.ts'] }, controlToken,
    status: async () => ({ pending: [...approvals.pending.values()], audit }), record: request => audit.push(request),
    control: async action => { if (action === 'approve') { approved.add(peer.id); approvals.approve(peer.id); } else if (action === 'revoke') { approved.delete(peer.id); approvals.deny(peer.id); host.revoke(peer.id); } },
  }); cleanups.push(relay.close);
  const clientId = randomUUID();
  const post = async (route: string, payload: unknown, options: { peer?: typeof peer; headers?: Record<string, string>; timestamp?: string } = {}) => {
    const body = JSON.stringify(payload), headers = signedHeaders(secret, options.peer ?? peer, route, body, options.timestamp);
    return (await fetch(relay.url + route, { method: 'POST', headers: { ...headers, ...options.headers }, body })).json();
  };
  const request = (method: string, payload: unknown) => post('/v1/request', { clientId, method, payload });
  const control = async (action: 'approve' | 'revoke') => { const result = await fetch(relay.url + '/__qa/control', { method: 'POST', headers: { Authorization: 'Bearer ' + controlToken, 'Content-Type': 'application/json' }, body: JSON.stringify({ action }) }); expect(result.status).toBe(200); };
  const pair = () => post('/v1/pair', { clientId });
  const connect = async () => { expect(await pair()).toMatchObject({ ok: false, error: { code: 'APPROVAL_REQUIRED' } }); await control('approve'); expect(await pair()).toMatchObject({ ok: true, value: { paired: true } }); };
  return { root, outside, directory, audit, approvals, secret, controlToken, relay, clientId, post, request, pair, control, connect };
}
it('requires real signed pairing and approval and never exposes the host-runner token to Android config', async () => {
  const f = await fixture();
  const config = await (await fetch(f.relay.url + '/__qa/config')).json();
  expect(config).toMatchObject({ root: f.root, hmacSecret: f.secret }); expect(config).not.toHaveProperty('controlToken');
  expect((await fetch(f.relay.url + '/__qa/status')).status).toBe(403);
  expect(await f.post('/v1/pair', { clientId: f.clientId }, { headers: { 'x-shellfox-signature': '0'.repeat(64) } })).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  expect(await f.post('/v1/pair', { clientId: f.clientId }, { timestamp: String(Date.now() - 60000) })).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  expect(await f.post('/v1/pair', { clientId: f.clientId }, { peer: { id: 'other-peer', hostname: 'Android emulator QA' } })).toMatchObject({ ok: false, error: { code: 'AUTH_FAILED' } });
  await f.connect();
  expect(await f.request('readFile', { path: path.join(f.root, 'code.ts') })).toMatchObject({ ok: true, value: { text: 'actual Windows test disk' } });
});
it('blocks home, traversal, outside reads/writes, symlink escapes and unrelated mutations without rewriting signed requests', async () => {
  const f = await fixture(); await f.connect();
  await symlink(f.outside, path.join(f.root, 'escape'), process.platform === 'win32' ? 'junction' : 'dir');
  for (const candidate of ['~', '.', path.join(f.outside, 'secret.txt'), path.join(f.root, '..', 'outside', 'secret.txt'), path.join(f.root, 'escape', 'secret.txt')]) expect(await f.request('readFile', { path: candidate })).toMatchObject({ ok: false, error: { code: 'ACCESS_DENIED' } });
  expect(await f.request('beginFileWrite', { path: path.join(f.root, 'escape', 'new.bin'), size: 0, mode: 'create', content: 'file' })).toMatchObject({ ok: false, error: { code: 'ACCESS_DENIED' } });
  for (const method of ['createSession', 'addTab', 'saveSettings', 'setTerminalEnv', 'createDirectory']) expect(await f.request(method, { path: f.root })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
  expect(await readFile(path.join(f.outside, 'secret.txt'), 'utf8')).toBe('must not leak'); expect(await readdir(f.outside)).toEqual(['secret.txt']);
  expect(f.audit.some(item => item.result === 'QA_ROOT_DENIED')).toBe(true);
});
it('routes owner-bound chunk handles and revokes partial uploads through the production host approval boundary', async () => {
  const f = await fixture(); await f.connect();
  const started = await f.request('beginFileWrite', { path: path.join(f.root, 'upload.bin'), size: 1, mode: 'create', content: 'file' });
  if (!started || typeof started !== 'object' || !('ok' in started) || started.ok !== true || !('value' in started) || !started.value || typeof started.value !== 'object' || !('transferId' in started.value)) throw new Error('Transfer missing');
  const transferId = started.value.transferId;
  const other = randomUUID(); expect(await f.post('/v1/pair', { clientId: other })).toMatchObject({ ok: true });
  expect(await f.post('/v1/request', { clientId: other, method: 'writeFileChunk', payload: { transferId, offset: 0, data: 'YQ==' } })).toMatchObject({ ok: false, error: { code: 'NOT_FOUND' } });
  expect(await f.request('writeFileChunk', { transferId, offset: 0, data: 'YQ==' })).toMatchObject({ ok: true });
  await f.control('revoke');
  expect(await f.request('finishFileWrite', { transferId })).toMatchObject({ ok: false, error: { code: 'ACCESS_DENIED' } });
  await vi.waitFor(async () => expect((await readdir(f.root)).some(name => name.startsWith('.shellfox-transfer-') || name === 'upload.bin')).toBe(false));
});
