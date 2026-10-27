import { expect, it } from 'vitest';
import { REMOTE_REQUEST_BYTES, remoteForwardRequestSchema } from './remote-schemas';

it('accepts a full terminal input frame even when JSON escaping expands it sixfold', () => {
  const request = { peerId: 'node', method: 'writeTerminal', payload: { tabId: crypto.randomUUID(), generation: crypto.randomUUID(), controlEpoch: 1, data: '\x1b'.repeat(65536) } };
  expect(new TextEncoder().encode(JSON.stringify(request)).byteLength).toBeGreaterThan(256 * 1024);
  expect(remoteForwardRequestSchema.safeParse(request).success).toBe(true);
});
it('rejects oversized environment updates before sending an uncertain remote mutation', () => {
  const request = { peerId: 'node', method: 'setSessionEnv', payload: { sessionId: crypto.randomUUID(), env: Array.from({ length: 10 }, (_, i) => ({ name: 'VALUE_' + i, value: '\x1b'.repeat(32767) })) } };
  expect(new TextEncoder().encode(JSON.stringify(request)).byteLength).toBeGreaterThan(REMOTE_REQUEST_BYTES);
  const result = remoteForwardRequestSchema.safeParse(request);
  expect(result.success).toBe(false);
  if (!result.success) expect(result.error.issues.some(issue => issue.message.includes('encoded size limit'))).toBe(true);
});
it('reserves protocol overhead at the encoded request boundary', () => {
  const request = (count: number) => ({ peerId: 'node', method: 'setSessionEnv', payload: { sessionId: crypto.randomUUID(), env: Array.from({ length: count }, (_, i) => ({ name: 'VALUE_' + i, value: 'x'.repeat(32767) })) } });
  expect(remoteForwardRequestSchema.safeParse(request(31)).success).toBe(true);
  expect(remoteForwardRequestSchema.safeParse(request(32)).success).toBe(false);
});
