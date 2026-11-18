// Rebuild deterministic host-auth vectors using Node's existing crypto contract.
import { createHash, createHmac } from 'node:crypto';
import { writeFileSync } from 'node:fs';
const peer = Buffer.from(JSON.stringify({ id: 'n-peer', hostname: 'my laptop' })).toString('base64url');
const clientId = '11111111-1111-4111-8111-111111111111';
const timestamp = '1700000000123';
const cases = [
  ['discover', '/v1/discover', '{}'],
  ['pair', '/v1/pair', JSON.stringify({ clientId })],
  ['snapshot', '/v1/request', JSON.stringify({ clientId, method: 'getSnapshot', payload: {} })],
  ['events', '/v1/events', JSON.stringify({ clientId })],
  ['disconnect', '/v1/disconnect', JSON.stringify({ clientId })],
  ['privileged', '/v1/request', JSON.stringify({ clientId, method: 'updateSettings', payload: {} })],
  ['invalid-json', '/v1/request', '{'],
  ['write', '/v1/request', JSON.stringify({ clientId, method: 'renameSession', payload: { sessionId: clientId, title: 'other' } })],
];
const vectors = cases.map(([name, path, body]) => ({ name, path, body, peer, timestamp, signature: createHmac('sha256', 'b'.repeat(32)).update([timestamp, peer, 'POST', path, createHash('sha256').update(body).digest('hex')].join('\n')).digest('hex') }));
writeFileSync(new URL('./fixtures/auth-vectors.json', import.meta.url), JSON.stringify(vectors, null, 2) + '\n');
