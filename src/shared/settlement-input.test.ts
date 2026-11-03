import { describe, expect, it } from 'vitest';
import { requestSchemas, terminalEventSchema } from './schemas';
import { isTerminalFocusReport } from './terminal-input';

const identity = { tabId: '11111111-1111-4111-8111-111111111111', generation: '22222222-2222-4222-8222-222222222222' };
const input = { ...identity, controlEpoch: 1, data: 'typed' };
const settledAt = '2026-11-13T12:00:00.000Z';

describe('settlement input markers', () => {
  it('recognizes only complete focus reports, including adjacent reports in one read', () => {
    for (const data of ['\x1b[I', '\x1b[O', '\x1b[O\x1b[I']) expect(isTerminalFocusReport(data)).toBe(true);
    for (const data of ['', 'typed', '\x1b[A', '\x1b[', '\x1b[Ityped', 'typed\x1b[O']) expect(isTerminalFocusReport(data)).toBe(false);
  });
  it('accepts legacy input without a marker and preserves explicit null or timestamp markers', () => {
    expect(requestSchemas.writeTerminal.parse(input)).toEqual(input);
    for (const marker of [null, settledAt]) {
      expect(requestSchemas.writeTerminal.parse({ ...input, settledAt: marker }).settledAt).toBe(marker);
    }
  });
  it.each(['yesterday', 123, {}, ''])('rejects invalid settlement marker %j', marker => {
    expect(requestSchemas.writeTerminal.safeParse({ ...input, settledAt: marker }).success).toBe(false);
  });
  it('validates generation-scoped settlement metadata without output credit or attachment state', () => {
    expect(terminalEventSchema.parse({ type: 'settlement', ...identity, settledAt })).toEqual({ type: 'settlement', ...identity, settledAt });
    expect(terminalEventSchema.safeParse({ type: 'settlement', ...identity, settledAt: null }).success).toBe(true);
    expect(terminalEventSchema.safeParse({ type: 'settlement', ...identity }).success).toBe(false);
  });
});
