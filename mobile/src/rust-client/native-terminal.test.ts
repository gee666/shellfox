import { describe, expect, it } from 'vitest';
import { nativeAttachmentSchema } from './native-terminal';
import { responseSchemas, resultSchema } from '../../../src/shared/schemas';
import ownedWire from '../../../crates/shellfox-client/tests/fixtures/terminal-attach-owned-success.json';
import blockedWire from '../../../crates/shellfox-client/tests/fixtures/terminal-attach-blocked-success.json';

// SOURCE-PROJECTED, not a device/native-execution capture. This is exactly
// client_terminal.rs::attachment in snapshot860d3aa1c8f4a5f5c234a4058156cc77e1da54e87b9251a8ba3de14595f8a50f:
// serde_json::to_value(TerminalAttachment), then remove only "chunks".
function projectedOwned() {
  const reply = responseSchemas.attachTerminal.parse(ownedWire);
  if (!reply.ok || reply.value.control !== 'owned') throw new Error('Invalid canonical owned fixture.');
  const { chunks: _chunks, ...metadata } = reply.value;
  return metadata;
}

describe('v1 native attachment serializer boundary', () => {
  it('accepts the exact source-projected canonical owned metadata including lifetime', () => {
    const metadata = projectedOwned();
    expect(metadata.lifetime).toBe('app-owned');
    expect(nativeAttachmentSchema.parse(metadata)).toEqual(metadata);
    expect(resultSchema(nativeAttachmentSchema).parse({ ok: true, value: metadata }))
      .toEqual({ ok: true, value: metadata });
    expect(Object.hasOwn(metadata, 'chunks')).toBe(false);
    expect(JSON.stringify(metadata)).not.toContain('hello');
  });

  it('keeps omitted, explicit-null and captured settlement distinct', () => {
    const { settledAt: _settledAt, ...metadata } = projectedOwned();
    const absent = nativeAttachmentSchema.parse(metadata);
    expect(Object.hasOwn(absent, 'settledAt')).toBe(false);
    for (const settledAt of [null, '2026-01-01T00:00:00.000Z']) {
      expect(nativeAttachmentSchema.parse({ ...metadata, settledAt }))
        .toEqual({ ...metadata, settledAt });
    }
  });

  it('rejects missing/false lifetime, raw output, invalid epochs and guessed nullable fields', () => {
    const metadata = projectedOwned();
    const { lifetime: _lifetime, ...missing } = metadata;
    for (const value of [missing, { ...metadata, lifetime: 'external-legacy' },
      { ...metadata, snapshot: null }, { ...metadata, chunks: [] },
      { ...metadata, data: 'raw output' }, { ...metadata, controlEpoch: 0 },
      { ...metadata, firstSequence: 0 }, { ...metadata, firstSequence: metadata.lastSequence + 2 }]) {
      expect(nativeAttachmentSchema.safeParse(value).success).toBe(false);
    }
  });

  it('retains the exact blocked shape without inventing owned metadata', () => {
    const reply = responseSchemas.attachTerminal.parse(blockedWire);
    if (!reply.ok || reply.value.control !== 'blocked') throw new Error('Invalid canonical blocked fixture.');
    expect(nativeAttachmentSchema.parse(reply.value)).toEqual(reply.value);
    expect(nativeAttachmentSchema.safeParse({ ...reply.value, lifetime: 'app-owned' }).success).toBe(false);
  });
});
