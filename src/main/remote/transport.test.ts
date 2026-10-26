import { afterEach, expect, it } from 'vitest';
import { createServer } from 'node:http';
import { proxyRequest, validateProxyUrl } from './transport';
const cleanup: (() => Promise<void>)[] = [];
afterEach(async () => { for (const close of cleanup.splice(0)) await close(); });
it('accepts only an explicit loopback HTTP port without credentials, paths, queries or fragments', () => {
  expect(validateProxyUrl({ proxyUrl: 'http://127.0.0.1:34567' })).toBe('http://127.0.0.1:34567');
  for (const proxyUrl of ['http://127.0.0.1', 'http://127.0.0.1:0', 'http://localhost:123', 'http://[::1]:123', 'https://127.0.0.1:123', 'http://user:pass@127.0.0.1:123', 'http://127.0.0.1:123/path', 'http://127.0.0.1:123/?secret=1', 'http://127.0.0.1:123/#secret', 'http://100.64.0.1:123']) expect(() => validateProxyUrl({ proxyUrl })).toThrow();
});
it('refuses redirects instead of forwarding authenticated proxy requests elsewhere', async () => {
  let requests = 0;
  const server = createServer((_request, response) => { requests++; response.writeHead(307, { Location: '/redirected' }); response.end(); });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  cleanup.push(async () => { server.closeAllConnections(); await new Promise<void>(resolve => server.close(() => resolve())); });
  const address = server.address(); if (!address || typeof address === 'string') throw new Error('server failed');
  await expect(proxyRequest(`http://127.0.0.1:${address.port}`, 'secret', 'peer', '/v1/pair', {})).rejects.toThrow();
  expect(requests).toBe(1);
});
