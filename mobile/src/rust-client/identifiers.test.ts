import { describe, expect, it } from 'vitest';
import { positiveDecimalSchema, unsignedDecimalSchema } from './identifiers';
import { progressSchema } from './adapter';

describe('lossless native integer validation', () => {
  it('rejects malformed/overflow IDs without throwing from safeParse', () => {
    for (const value of ['', '-1', '01', '1.5', 'pointer:0x123', '18446744073709551616', '9'.repeat(1000)]) {
      expect(positiveDecimalSchema.safeParse(value).success).toBe(false);
      expect(unsignedDecimalSchema.safeParse(value).success).toBe(false);
      expect(progressSchema.safeParse({ acceptedSequence: value, consumedSequence: value,
        queuedBytes: 0, queuedChunks: 0 }).success).toBe(false);
    }
    expect(positiveDecimalSchema.safeParse('0').success).toBe(false);
    expect(unsignedDecimalSchema.parse('0')).toBe('0');
    expect(positiveDecimalSchema.parse('18446744073709551615')).toBe('18446744073709551615');
  });
});
