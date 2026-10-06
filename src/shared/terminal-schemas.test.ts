import { it, expect } from 'vitest';
import { randomUUID } from 'node:crypto';
import { requestSchemas, terminalEventSchema, terminalAttachmentSchema } from './schemas';
const identity = () => ({ tabId: randomUUID(), generation: randomUUID() });
it('strictly validates every terminal control method and rejects arbitrary launch templates', () => {
  const id = identity();
  for (const [method, payload] of [
    ['writeTerminal', { ...id, data: 'ls\r' }], ['resizeTerminal', { ...id, cols: 80, rows: 24 }], ['closeTab', id],
    ['attachTerminal', { tabId: id.tabId, afterSequence: 0 }], ['acknowledgeTerminal', { ...id, sequence: 0 }], ['detachTerminal', id], ['getTerminalProfiles', {}],
  ] as const) {
    expect(requestSchemas[method].safeParse(payload).success).toBe(true);
    expect(requestSchemas[method].safeParse({ ...payload, command: 'arbitrary' }).success).toBe(false);
  }
  expect(requestSchemas.addTab.safeParse({ sessionId: randomUUID(), profileId: 'wsl:Ubuntu', cwd: '/home/user' }).success).toBe(true);
  expect(requestSchemas.addTab.safeParse({ sessionId: randomUUID(), executable: '/evil', args: [] }).success).toBe(false);
});
it('bounds bytes rather than only character counts, and accepts VT controls in output', () => {
  const id = identity();
  expect(requestSchemas.writeTerminal.safeParse({ ...id, data: '雪'.repeat(22000) }).success).toBe(false);
  expect(requestSchemas.writeTerminal.safeParse({ ...id, data: '\0' }).success).toBe(false);
  expect(terminalEventSchema.safeParse({ type: 'data', ...id, sequence: 1, data: '\x1b[31m\0\r\n' }).success).toBe(true);
  expect(terminalEventSchema.safeParse({ type: 'data', ...id, sequence: 1, data: '雪'.repeat(5500) }).success).toBe(false);
});
it('rejects duplicate/out-of-order replay, mixed identities and dishonest lifetime declarations', () => {
  const id = identity(), chunk = { type: 'data' as const, ...id, sequence: 1, data: 'x' };
  const attachment = { ...id, sessionId: randomUUID(), firstSequence: 1, lastSequence: 1, chunks: [chunk], truncated: false, state: 'open', exitCode: null, cols: 80, rows: 24, lifetime: 'app-owned' };
  expect(terminalAttachmentSchema.safeParse(attachment).success).toBe(true);
  expect(terminalAttachmentSchema.safeParse({ ...attachment, chunks: [chunk, chunk] }).success).toBe(false);
  expect(terminalAttachmentSchema.safeParse({ ...attachment, chunks: [{ ...chunk, generation: randomUUID() }] }).success).toBe(false);
  expect(terminalAttachmentSchema.safeParse({ ...attachment, lifetime: 'detached' }).success).toBe(false);
  expect(terminalAttachmentSchema.safeParse({ ...attachment, chunks: Array.from({ length: 70 }, (_, i) => ({ ...chunk, sequence: i + 1, data: 'x'.repeat(4096) })), lastSequence: 70 }).success).toBe(false);
});
