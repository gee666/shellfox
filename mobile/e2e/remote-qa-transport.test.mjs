import assert from 'node:assert/strict';
import { createHash, createHmac } from 'node:crypto';
import { test } from 'node:test';
import { createQaRemoteTransport, qaRemoteConfigSchema, qaSignedHeaders, QA_REMOTE_ORIGIN } from './remote-qa-transport.ts';
const config = qaRemoteConfigSchema.parse({ protocol: 1, hostId: 'windows-real-host-qa', clientPeer: { id: 'android-real-files-qa', hostname: 'Android emulator QA' }, hmacSecret: 'synthetic-qa-only-secret-not-user-auth', root: 'P:/tmp/qa/files', files: [] });
test('pure JS HMAC matches production Node SHA256 for exact UTF-8 request bytes', () => {
  for (const body of ['{}', JSON.stringify({ text: 'café 日本語' }), JSON.stringify({ data: 'A'.repeat(256 * 1024) })]) {
    const timestamp = '1793926289000', headers = qaSignedHeaders(config, '/v1/request', body, timestamp);
    const peer = Buffer.from(JSON.stringify(config.clientPeer)).toString('base64url');
    const message = [timestamp, peer, 'POST', '/v1/request', createHash('sha256').update(body).digest('hex')].join('\n');
    assert.equal(headers['x-shellfox-peer'], peer); assert.equal(headers['x-shellfox-signature'], createHmac('sha256', config.hmacSecret).update(message).digest('hex'));
  }
});
test('QA transport only routes its fixed peer, fixed origin and production remote paths', async () => {
  const calls = [];
  const transport = createQaRemoteTransport(config, async (url, options) => { calls.push({ url, options }); return new Response('{"ok":true}'); });
  assert.equal(await transport.request(config.hostId, '/v1/pair', '{}'), '{"ok":true}');
  assert.equal(calls[0].url, QA_REMOTE_ORIGIN + '/v1/pair');
  await assert.rejects(transport.request('user-peer', '/v1/pair', '{}'));
  await assert.rejects(transport.request(config.hostId, '/__qa/control', '{}'));
  assert.equal(calls.length, 1);
});
