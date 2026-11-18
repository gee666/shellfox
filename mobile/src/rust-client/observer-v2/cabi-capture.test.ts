import { lstatSync, readFileSync, realpathSync } from 'node:fs';
import { basename, dirname, resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { z } from 'zod';
import { bindingSchema, boundReply, reply, discoverySchema, pairOfferSchema, pairSelectionSchema } from './primitives';
import { attachmentSchema, nativeAttachmentSchema } from './delivery';
import { nativeObserverEventSchema } from './native';
import { observerMethods, deniedMethods, observerValueSchemas, requestEnvelopeSchema } from './methods';

// Explicit test-only handoff. No capture is read during normal app/test startup.
const captureFile = process.env.SHELLFOX_OBSERVER_CABI_CAPTURE;
const fixture = (name: string): unknown => JSON.parse(readFileSync(resolve(
  'crates/shellfox-protocol/tests/fixtures/observer_v2', name + '.json'), 'utf8'));
const rowsSchema = z.array(z.object({ method: z.enum(observerMethods), allowed: z.boolean(),
  request: requestEnvelopeSchema, response: z.unknown() }).strict()).length(43);
const traceSchema = z.object({ request: z.unknown(), event: nativeObserverEventSchema }).strict();
const captureSchema = z.object({
  captureVersion: z.literal(1), wireVersion: z.literal(2), profile: z.literal('observer-v2'),
  observerAbiVersion: z.literal(1), legacyAbiVersion: z.literal(1),
  handshake: z.array(z.object({ kind: z.enum(['discover', 'pair']), request: z.unknown(), event: nativeObserverEventSchema }).strict()).length(2),
  nativeAcks: z.array(traceSchema).min(1).max(43),
  vectors: z.array(z.discriminatedUnion('accepted', [
    z.object({ method: z.enum(observerMethods), accepted: z.literal(false), status: z.literal(2) }).strict(),
    z.object({ method: z.enum(observerMethods), accepted: z.literal(true), request: requestEnvelopeSchema,
      event: nativeObserverEventSchema, expected: z.unknown() }).strict(),
  ])).length(43),
}).strict();

function readCapture() {
  if (!captureFile) throw new Error('Opt-in capture path is required.');
  const requested = resolve(captureFile);
  const root = realpathSync(resolve('tmp/observer-v2-crosslang'));
  const stat = lstatSync(requested);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size > 16 * 1024 * 1024
    || dirname(realpathSync(requested)) !== root || !/^capture-[0-9a-f-]+\.json$/.test(basename(requested))) {
    throw new Error('Capture must be an owned bounded fixture file under root tmp/observer-v2-crosslang.');
  }
  return captureSchema.parse(JSON.parse(readFileSync(requested, 'utf8')));
}

describe.skipIf(!captureFile)('opt-in actual observer C ABI to TS cross-language proof', () => {
  it('preserves all 43 method keys/nulls/facts and actual recovery consumption ACKs', () => {
    const capture = readCapture();
    const goldens = rowsSchema.parse(fixture('methods'));
    expect(new Set(capture.vectors.map(row => row.method))).toEqual(new Set(observerMethods));
    const expectedBinding = bindingSchema.parse(fixture('binding'));
    let rejected = 0;
    for (const row of capture.vectors) {
      const golden = goldens.find(value => value.method === row.method);
      if (!golden) throw new Error('Missing canonical method.');
      if (!row.accepted) {
        expect(new Set<string>([...deniedMethods, 'acknowledgeTerminal']).has(row.method)).toBe(true);
        expect(row.status).toBe(2); rejected++; continue;
      }
      expect(golden.allowed).toBe(true);
      expect(row.method).not.toBe('acknowledgeTerminal');
      expect(row.request).toEqual(golden.request);
      expect(row.event.status).toBe(0); expect(row.event.internal).toBe(false);
      expect(row.event.connection.binding).toEqual(expectedBinding);
      const wire = boundReply(observerValueSchemas[row.method]).parse(golden.response);
      // The test's expected value is independently derived from canonical bytes,
      // not trusted from the Rust capture's convenience `expected` property.
      expect(JSON.parse(JSON.stringify(wire)), row.method).toEqual(golden.response);
      const { binding: _binding, ...expected } = wire;
      let nativeExpected: unknown = expected;
      if (row.method === 'attachTerminal' && wire.ok) {
        const attached = attachmentSchema.parse(wire.value);
        if (attached.kind !== 'attached') throw new Error('Expected attached canonical fixture.');
        const { data: _data, ...recovery } = attached.recovery;
        nativeExpected = { version: 2, ok: true, value: { ...attached, recovery } };
        reply(nativeAttachmentSchema).parse(row.event.result);
      } else {
        reply(observerValueSchemas[row.method]).parse(row.event.result);
      }
      // ONLY the documented native carrier projection is allowed: binding moves
      // to connection, and recovery bytes remain native. Never filter other keys.
      expect(row.event.result, row.method).toEqual(nativeExpected);
      expect(row.expected, row.method).toEqual(nativeExpected);
    }
    expect(rejected).toBe(8);
    const discovery = reply(discoverySchema).parse(fixture('discovery'));
    if (!discovery.ok) throw new Error('Invalid discovery fixture.');
    expect(capture.handshake[0].request).toEqual(fixture('discovery-request'));
    expect(capture.handshake[0].event.result).toEqual({ kind: 'available', discovery: discovery.value });
    expect(pairOfferSchema.parse(capture.handshake[1].request)).toEqual(fixture('pair-offer'));
    expect(reply(pairSelectionSchema).parse(capture.handshake[1].event.result)).toEqual(fixture('pair-selection'));
    expect(capture.handshake[1].event.connection.binding).toEqual(expectedBinding);
    const ack = capture.nativeAcks[0];
    expect(requestEnvelopeSchema.parse(ack.request)).toEqual(fixture('request-ack-recovery'));
    expect(ack.event.internal).toBe(true); expect(ack.event.status).toBe(0);
    const acknowledged = boundReply(z.object({ acknowledged: z.literal(true) }).strict()).parse(fixture('acknowledged'));
    const { binding: _binding, ...expectedAck } = acknowledged;
    expect(ack.event.result).toEqual(expectedAck);
    expect(ack.event.connection.binding).toEqual(expectedBinding);
  });
});
