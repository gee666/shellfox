import { createHash, createHmac, timingSafeEqual } from 'node:crypto';
import type { IncomingHttpHeaders } from 'node:http';
import { z } from 'zod';
import { remotePeerIdSchema } from '../../shared/remote-schemas';
export const peerIdentitySchema = z.object({ id: remotePeerIdSchema, hostname: z.string().min(1).max(253).refine(s => !/[\x00-\x1f\x7f]/.test(s)) }).strict();
export function verifyPeer(secret: string, headers: IncomingHttpHeaders, method: string, url: string, body: Buffer, now = Date.now()): z.infer<typeof peerIdentitySchema> | null {
  const peer = headers['x-shellfox-peer'], timestamp = headers['x-shellfox-timestamp'], signature = headers['x-shellfox-signature'];
  if (typeof peer !== 'string' || peer.length > 2048 || typeof timestamp !== 'string' || !/^\d{13}$/.test(timestamp) || Math.abs(now - Number(timestamp)) > 30_000 || typeof signature !== 'string' || !/^[a-f0-9]{64}$/.test(signature)) return null;
  const message = [timestamp, peer, method, url, createHash('sha256').update(body).digest('hex')].join('\n');
  const expected = createHmac('sha256', secret).update(message).digest();
  if (!timingSafeEqual(expected, Buffer.from(signature, 'hex'))) return null;
  try { return peerIdentitySchema.parse(JSON.parse(Buffer.from(peer, 'base64url').toString('utf8'))); } catch { return null; }
}
