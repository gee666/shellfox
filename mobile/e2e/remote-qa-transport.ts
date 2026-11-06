// Only the explicit real-device QA entry imports this loopback transport.
import { z } from 'zod';
import { sha256 } from '@noble/hashes/sha2.js';
import { hmac } from '@noble/hashes/hmac.js';
import { bytesToHex, utf8ToBytes } from '@noble/hashes/utils.js';
import type { NativeRemoteTransport, RemotePath } from '../src/client';

export const QA_REMOTE_ORIGIN = 'http://10.0.2.2:41973';
export const qaRemoteConfigSchema = z.object({
  protocol: z.literal(1), hostId: z.literal('windows-real-host-qa'),
  clientPeer: z.object({ id: z.literal('android-real-files-qa'), hostname: z.string().min(1).max(100) }).strict(),
  hmacSecret: z.string().min(16).max(256), root: z.string().min(3).max(32767), files: z.array(z.string()).max(100),
}).strict();
export type QaRemoteConfig = z.infer<typeof qaRemoteConfigSchema>;
const routes = new Set<RemotePath>(['/v1/pair', '/v1/request', '/v1/events', '/v1/disconnect']);
export function qaSignedHeaders(config: QaRemoteConfig, route: RemotePath, body: string, timestamp = String(Date.now())) {
  const encoded = btoa(Array.from(utf8ToBytes(JSON.stringify(config.clientPeer)), value => String.fromCharCode(value)).join('')).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  const message = [timestamp, encoded, 'POST', route, bytesToHex(sha256(utf8ToBytes(body)))].join('\n');
  return { 'Content-Type': 'application/json', 'x-shellfox-peer': encoded, 'x-shellfox-timestamp': timestamp,
    'x-shellfox-signature': bytesToHex(hmac(sha256, utf8ToBytes(config.hmacSecret), utf8ToBytes(message))) };
}
export async function loadQaRemoteConfig(): Promise<QaRemoteConfig> {
  const response = await fetch(`${QA_REMOTE_ORIGIN}/__qa/config`);
  if (!response.ok) throw new Error('The isolated Windows QA host is not running.');
  return qaRemoteConfigSchema.parse(await response.json());
}
export function createQaRemoteTransport(config: QaRemoteConfig, request: typeof fetch = fetch): NativeRemoteTransport {
  return { request: async (peerId, route, body) => {
    if (peerId !== config.hostId || !routes.has(route)) throw new Error('QA transport only accepts its fixed Windows peer and production remote routes.');
    const controller = new AbortController(), timer = setTimeout(() => controller.abort(), 30_000);
    try {
      const response = await request(QA_REMOTE_ORIGIN + route, { method: 'POST', body, headers: qaSignedHeaders(config, route, body), signal: controller.signal });
      if (!response.ok) throw new Error(`Windows QA host HTTP ${response.status}`);
      return await response.text();
    } finally { clearTimeout(timer); }
  } };
}
