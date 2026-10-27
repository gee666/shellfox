import { z } from 'zod';
const proxyUrlSchema = z.url().refine(value => { const url = new URL(value); return url.protocol === 'http:' && url.hostname === '127.0.0.1' && Number(url.port) > 0 && !url.username && !url.password && url.pathname === '/' && !url.search && !url.hash; });
export function validateProxyUrl(raw: unknown): string {
  const parsed = z.object({ proxyUrl: proxyUrlSchema }).parse(raw);
  return parsed.proxyUrl.replace(/\/$/, '');
}
export class ProxyResponseError extends Error {
  constructor(readonly status: number) { super('Remote host is unavailable.'); }
}
export async function proxyRequest(proxyUrl: string, proxySecret: string, peerId: string, path: string, body: unknown, signal?: AbortSignal, maxBytes = 8 * 1024 * 1024): Promise<unknown> {
  const response = await fetch(proxyUrl + '/proxy', { method: 'POST', redirect: 'error', headers: { Authorization: 'Bearer ' + proxySecret, 'Content-Type': 'application/json' }, body: JSON.stringify({ peerId, path, body }), signal: signal ? AbortSignal.any([signal, AbortSignal.timeout(30_000)]) : AbortSignal.timeout(30_000) });
  if (!response.ok || !response.body) { await response.body?.cancel(); throw new ProxyResponseError(response.status); }
  const reader = response.body.getReader(); const chunks: Uint8Array[] = []; let bytes = 0;
  try {
    while (true) {
      const next = await reader.read(); if (next.done) break;
      bytes += next.value.length; if (bytes > maxBytes) throw new Error('Remote response is too large.');
      chunks.push(next.value);
    }
  } finally { await reader.cancel().catch(() => {}); }
  return JSON.parse(Buffer.concat(chunks).toString('utf8'));
}
