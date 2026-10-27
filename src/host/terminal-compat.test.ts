import { afterEach, expect, it } from 'vitest';
import { createServer, type Server } from 'node:http';
import { randomUUID } from 'node:crypto';
import type { HostEndpoint } from '../shared/host-contracts';
import { hostEnvelopeSchema } from '../shared/host-schemas';
import { embeddedFixture } from '../main/remote/test-fixtures';
import { hostRequest } from './client-transport';
const servers: Server[] = [], cleanups: Array<() => Promise<void>> = [];
afterEach(async () => { for (const server of servers.splice(0)) { server.closeAllConnections(); await new Promise<void>(resolve => server.close(() => resolve())); } for (const close of cleanups.splice(0)) await close(); });
it('turns an older local Host RPC raw attachment into an explicit upgrade error while metadata remains usable', async () => {
  const f = await embeddedFixture(); cleanups.push(() => f.service.dispose());
  const session = await f.service.createSession({ cwd: '/work', requestId: randomUUID() });
  if (!session.ok) throw new Error('No session');
  const tabId = session.value.tabs[0].id;
  f.processes[0].output('legacy private screen');
  const server = createServer((request, response) => { void (async () => {
    const chunks: Buffer[] = []; for await (const chunk of request) chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
    const input = hostEnvelopeSchema.parse(JSON.parse(Buffer.concat(chunks).toString('utf8')));
    const value = input.method === 'attachTerminal' ? f.backend.attach({ tabId }) : f.service.getSnapshot();
    response.writeHead(200, { 'Content-Type': 'application/json' }); response.end(JSON.stringify({ requestId: input.requestId, ok: true, value }));
  })().catch(() => { response.writeHead(500); response.end(); }); });
  servers.push(server); await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const address = server.address(); if (!address || typeof address === 'string') throw new Error('No local host');
  const endpoint: HostEndpoint = { version: 1, host: '127.0.0.1', port: address.port, secret: '0'.repeat(64), mode: 'headless' };
  const attached = await hostRequest(endpoint, randomUUID(), 'attachTerminal', { tabId });
  expect(attached).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED', message: 'Upgrade the host to use exclusive terminal control.', retryable: false } });
  expect(JSON.stringify(attached)).not.toContain('legacy private screen');
  expect(await hostRequest(endpoint, randomUUID(), 'getSnapshot', {})).toMatchObject({ ok: true, value: { sessions: [expect.objectContaining({ id: session.value.id })] } });
});
