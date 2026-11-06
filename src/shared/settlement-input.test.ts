import { describe, expect, it } from 'vitest';
import { requestSchemas, terminalEventSchema } from './schemas';
import { isPassiveTerminalInput } from './terminal-input';

const identity = { tabId: '11111111-1111-4111-8111-111111111111', generation: '22222222-2222-4222-8222-222222222222' };
const input = { ...identity, controlEpoch: 1, data: 'typed' };
const settledAt = '2026-11-13T12:00:00.000Z';

describe('settlement input markers', () => {
  it('recognizes only complete focus reports, including adjacent reports in one read', () => {
    for (const data of ['\x1b[I', '\x1b[O', '\x1b[O\x1b[I']) expect(isPassiveTerminalInput(data)).toBe(true);
    for (const data of ['', 'typed', '\x1b[A', '\x1b[', '\x1b[Ityped', 'typed\x1b[O']) expect(isPassiveTerminalInput(data)).toBe(false);
  });
  it('recognizes hover reports in SGR, pixel and legacy encoding with every modifier', () => {
    for (const button of [35, 39, 43, 47, 51, 55, 59, 63]) {
      const sgr = `\x1b[<${button};123;456M`;
      const legacy = `\x1b[M${String.fromCharCode(button + 32)}AB`;
      for (const data of [sgr, legacy, `${sgr}${legacy}\x1b[O\x1b[I`]) expect(isPassiveTerminalInput(data)).toBe(true);
    }
  });
  it('keeps clicks, releases, dragging, wheel, keys, pasted text and incomplete reports active', () => {
    const hover = '\x1b[<35;12;8M';
    for (const data of [
      '\x1b[<0;12;8M', '\x1b[<0;12;8m', '\x1b[<32;12;8M', '\x1b[<64;12;8M', '\x1b[<65;12;8M',
      '\x1b[<99;12;8M', '\x1b[<35;12;8m', '\x1b[<35;12;', '\x1b[MCA', '\x1b[M AB', '\x1b[M@AB',
      '\x1b[MC\u0100\u0101', '\x1b[MCA\u0100',
      '\x1b[200~paste\x1b[201~', '\x1b[A', '\x03', 'typed',
    ]) {
      expect(isPassiveTerminalInput(data)).toBe(false);
      expect(isPassiveTerminalInput(hover + data)).toBe(false);
      expect(isPassiveTerminalInput(data + hover)).toBe(false);
    }
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
