import { expect, it } from 'vitest';
import { verifyPeer } from './auth';
import { signedHeaders, identity } from './test-fixtures';
it('binds identity, timestamp, method, path and exact body bytes to the helper secret', () => {
  const body = '{"clientId":"request"}', secret = 'backend-secret', path = '/v1/pair';
  const headers = signedHeaders(secret, identity, path, body);
  expect(verifyPeer(secret, headers, 'POST', path, Buffer.from(body))).toEqual(identity);
  expect(verifyPeer('wrong', headers, 'POST', path, Buffer.from(body))).toBeNull();
  expect(verifyPeer(secret, headers, 'GET', path, Buffer.from(body))).toBeNull();
  expect(verifyPeer(secret, headers, 'POST', '/v1/request', Buffer.from(body))).toBeNull();
  expect(verifyPeer(secret, headers, 'POST', path, Buffer.from(body + ' '))).toBeNull();
  expect(verifyPeer(secret, { ...headers, 'x-shellfox-peer': Buffer.from(JSON.stringify({ id: 'other', hostname: 'other' })).toString('base64url') }, 'POST', path, Buffer.from(body))).toBeNull();
  const now = Date.now();
  for (const delta of [-30_001, 30_001]) {
    const stale = signedHeaders(secret, identity, path, body, String(now + delta));
    expect(verifyPeer(secret, stale, 'POST', path, Buffer.from(body), now)).toBeNull();
  }
});
