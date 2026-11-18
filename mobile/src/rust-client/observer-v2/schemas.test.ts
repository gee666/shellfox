import { readFileSync, readdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { z } from 'zod';
import * as p from './primitives';
import * as d from './delivery';
import { snapshotSchema, catalogEntrySchema } from './catalog';
import { observerMethods, deniedMethods, observerValueSchemas, requestEnvelopeSchema } from './methods';
import { nativeObserverEventSchema, clientProfileSchema } from './native';
import { observerEnvironment } from './values';

const directory = resolve('crates/shellfox-protocol/tests/fixtures/observer_v2');
export const golden = (name: string): unknown => JSON.parse(readFileSync(resolve(directory, name + '.json'), 'utf8'));
const methodRows = z.array(z.object({ method: z.enum(observerMethods), allowed: z.boolean(),
  request: requestEnvelopeSchema, response: z.unknown() }).strict());
const schemas: Record<string, z.ZodType> = {
  'acknowledged': p.boundReply(z.object({ acknowledged: z.literal(true) }).strict()),
  'attachment': p.boundReply(d.attachmentSchema), 'binding': p.bindingSchema,
  'disconnect-attempt': p.disconnectSchema, 'disconnect-connection': p.disconnectSchema,
  'disconnected': p.reply(p.disconnectedSchema), 'discovery-request': p.discoverRequestSchema,
  'discovery': p.reply(p.discoverySchema), 'events-request': d.pollRequestSchema,
  'events': p.boundReply(d.pollSchema), 'layout': d.terminalEventSchema,
  'pair-offer': p.pairOfferSchema, 'pair-selection': p.reply(p.pairSelectionSchema),
  'process-exited': d.terminalEventSchema, 'profile-error': p.reply(p.discoverySchema),
  'request-ack-data': requestEnvelopeSchema, 'request-ack-recovery': requestEnvelopeSchema,
  'request-attach': requestEnvelopeSchema, 'request-detach': requestEnvelopeSchema,
  'request-snapshot': requestEnvelopeSchema, 'snapshot': p.boundReply(snapshotSchema),
  'source-ended': d.terminalEventSchema, 'methods': methodRows,
};
const selections = { version: 2, profile: 'observer-v2', catalogMode: 'complete',
  deliveryFormat: 'vt-bytes-base64-v1', recoveryFormat: 'formatted-active-buffer-v1' };

describe('observer-v2 separate TS contract', () => {
  it('validates all 24 canonical goldens without changing v1 fixtures', () => {
    const names = readdirSync(directory).filter(name => name.endsWith('.json'));
    expect(names).toHaveLength(24);
    expect(Object.keys(schemas)).toHaveLength(23);
    for (const [name, schema] of Object.entries(schemas)) {
      const body = golden(name);
      expect(schema.safeParse(body).success, name).toBe(true);
      expect(JSON.parse(JSON.stringify(schema.parse(body))), name).toEqual(body);
    }
    const contract = z.object({ ...Object.fromEntries(Object.entries(selections).map(([key, value]) => [key, z.literal(value)])),
      routes: z.object({ discover: z.literal('/v2/discover'), pair: z.literal('/v2/pair'), request: z.literal('/v2/request'), events: z.literal('/v2/events'), disconnect: z.literal('/v2/disconnect') }).strict(),
      methods: z.array(z.enum(observerMethods)).length(43), profileDeniedMethods: z.array(z.enum(deniedMethods)).length(7),
      limits: z.object({ requestBytes: z.literal(1048576), responseBytes: z.literal(8388608), recoveryDecodedBytes: z.literal(4194304),
        dataDecodedBytes: z.literal(16384), pollDecodedBytes: z.literal(262144), eventsPerPoll: z.literal(1024), deadlineMs: z.literal(30000),
        longPollMs: z.literal(20000), clientOperations: z.literal(8), clientPolls: z.literal(1), goSharedOperations: z.literal(12), goSharedPolls: z.literal(64) }).strict(),
    }).strict().parse(golden('contract'));
    expect(new Set(contract.methods)).toEqual(new Set(observerMethods));
    expect(new Set(contract.profileDeniedMethods)).toEqual(new Set(deniedMethods));
    for (const row of methodRows.parse(golden('methods'))) {
      expect(row.request.method).toBe(row.method);
      expect(row.allowed).toBe(!new Set<string>(deniedMethods).has(row.method));
      const response = p.boundReply(observerValueSchemas[row.method]);
      expect(response.safeParse(row.response).success, row.method).toBe(true);
      expect(JSON.parse(JSON.stringify(requestEnvelopeSchema.parse(row.request))), row.method).toEqual(row.request);
      expect(JSON.parse(JSON.stringify(response.parse(row.response))), row.method).toEqual(row.response);
    }
  });

  it('retains complete inventory and explicit unknown/not-applicable facts, without invented timestamps', () => {
    const parsed = p.boundReply(snapshotSchema).parse(golden('snapshot'));
    if (!parsed.ok) throw new Error('Golden snapshot is not successful.');
    expect(parsed.value.coverage).toEqual({ mode: 'complete', entryCount: 3 });
    expect(parsed.value.entries).toHaveLength(3);
    const raw = parsed.value.entries.find(entry => entry.kind === 'session'
      && entry.value.facts.createdAt.state === 'unknown');
    expect(raw?.kind).toBe('session');
    if (raw?.kind !== 'session') throw new Error('Missing raw source session.');
    expect(raw.value.facts.createdAt).toEqual({ state: 'unknown', reason: 'notRecorded' });
    expect(parsed.value.entries.some(entry => entry.kind === 'unavailable')).toBe(true);
    expect(p.fact(z.string().default('invented')).safeParse({ state: 'known' }).success).toBe(false);
    expect(p.fact(z.string().nullable()).parse({ state: 'known', value: null })).toEqual({ state: 'known', value: null });
    expect(catalogEntrySchema.safeParse({ kind: 'unavailable', sessionId: raw.value.id, issues: [] }).success).toBe(false);
  });

  it('preserves every optional settings omission and explicit null without v1 default insertion', () => {
    const row = methodRows.parse(golden('methods')).find(row => row.method === 'setTerminalEnv');
    if (!row) throw new Error('Missing settings fixture.');
    const result = p.boundReply(observerValueSchemas.setTerminalEnv).parse(row.response);
    if (!result.ok) throw new Error('Invalid settings fixture.');
    expect(Object.hasOwn(result.value, 'pythonPath')).toBe(false);
    const optional = new Set(['pythonPath', 'terminalEnv', 'autoUpdate', 'backgroundColor',
      'historyPageSize', 'sessionSettleHours', 'sessionDeleteHours', 'terminalProfileId']);
    const absent = Object.fromEntries(Object.entries(result.value).filter(([field]) => !optional.has(field)));
    expect(observerValueSchemas.setTerminalEnv.parse(absent)).toEqual(absent);
    const knownAbsent = { ...absent, pythonPath: null, terminalProfileId: null };
    expect(observerValueSchemas.setTerminalEnv.parse(knownAbsent)).toEqual(knownAbsent);
    expect(p.fact(observerValueSchemas.setTerminalEnv).safeParse({ state: 'known' }).success).toBe(false);
    const provenEnvironment = [{ name: ' FOO ', value: 'actual host value' }];
    expect(observerEnvironment(true).parse(provenEnvironment)).toEqual(provenEnvironment);
    expect(observerEnvironment(true).safeParse([...provenEnvironment, { name: 'FOO', value: 'duplicate' }]).success).toBe(false);
  });

  it('separates wire delivery from coarse native events and denies v1 lease shapes', () => {
    const attached = p.boundReply(d.attachmentSchema).parse(golden('attachment'));
    if (!attached.ok || attached.value.kind !== 'attached') throw new Error('Missing attached fixture.');
    expect(attached.value.input).toBeNull();
    expect(attached.value.layout).toEqual({ kind: 'fixedCanonicalGrid' });
    expect(d.nativeAttachmentSchema.safeParse(attached.value).success).toBe(false);
    const { data: _data, ...recovery } = attached.value.recovery;
    const metadata = { ...attached.value, recovery };
    expect(d.nativeAttachmentSchema.parse(metadata)).toEqual(metadata);
    expect(d.nativeAttachmentSchema.safeParse({ ...metadata, input: { controlEpoch: 1 } }).success).toBe(false);
    const events = p.boundReply(d.pollSchema).parse(golden('events'));
    if (!events.ok) throw new Error('Missing events fixture.');
    expect(d.coarsePollSchema.safeParse({ version: 2, ok: true, value: events.value }).success).toBe(false);
    expect(d.terminalEventSchema.parse(golden('source-ended')).type).toBe('sourceEnded');
    expect(d.terminalEventSchema.parse(golden('process-exited')).type).toBe('processExited');
    expect(d.recoverySchema.safeParse({ ...attached.value.recovery, data: 'GBth' }).success).toBe(false);
    expect(p.reply(p.discoverySchema).safeParse({ version: 1, ok: false, error: { code: 'UNSUPPORTED' } }).success).toBe(false);
    expect(clientProfileSchema.parse({ profile: 2, apiCapabilities: 1 })).toEqual({ profile: 2, apiCapabilities: 1 });
    expect(clientProfileSchema.safeParse({ profile: 2, apiCapabilities: 7 }).success).toBe(false);
  });

  it('accepts actual observer native metadata, never v1 permissions or raw data', () => {
    const binding = p.bindingSchema.parse(golden('binding'));
    const event = { profile: 'observer-v2', operationId: '1', internal: true, status: 0,
      result: { version: 2, ok: true, value: { events: [] } }, connection: { status: 'connected', binding },
      resetTickets: { '100': '1' }, resetGeometry: { '100': { cols: 200, rows: 80 } }, recoveryReady: ['100'] };
    expect(nativeObserverEventSchema.parse(event)).toEqual(event);
    expect(nativeObserverEventSchema.safeParse({ ...event, connection: { status: 'connected', binding, permission: 'approved' } }).success).toBe(false);
    expect(nativeObserverEventSchema.safeParse({ ...event, data: 'raw terminal output' }).success).toBe(false);
  });
});
