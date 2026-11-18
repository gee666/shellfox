import { z } from 'zod';
import { canonicalBase64 } from '../../../../src/shared/file-base64';
import { changedEventSchema } from '../../../../src/shared/schemas';
import { observerId, sequence, epoch, integer, observerErrorSchema, bindingSchema, reply } from './primitives';

export const decodedSize = (value: string): number => value.length / 4 * 3
  - (value.endsWith('==') ? 2 : value.endsWith('=') ? 1 : 0);
function encodedBytes(max: number) {
  return z.string().min(1).max(Math.ceil(max / 3) * 4)
    .refine(value => canonicalBase64(value) && decodedSize(value) <= max);
}
export const deliveryRefSchema = z.object({ tabId: observerId, generation: observerId,
  deliveryId: observerId, deliveryEpoch: epoch }).strict();
export const attachRequestSchema = z.object({ tabId: observerId, resume: z.object({
  generation: observerId, afterSequence: sequence,
}).strict().nullable() }).strict();
const recoveryFields = { recoveryId: observerId, format: z.literal('formatted-active-buffer-v1'),
  encoding: z.literal('base64'), lastSequence: sequence, cols: z.number().int().min(2).max(500),
  rows: z.number().int().min(2).max(500), layoutRevision: epoch };
export const recoverySchema = z.object({ ...recoveryFields,
  data: encodedBytes(4 * 1024 * 1024).refine(value => value.startsWith('GBtj')),
}).strict();
export const nativeRecoverySchema = z.object(recoveryFields).strict();
const fixedLayout = z.object({ kind: z.literal('fixedCanonicalGrid') }).strict();
const unavailable = z.object({ kind: z.literal('unavailable'), tabId: observerId,
  reason: z.enum(['sourceUnavailable', 'factsUnavailable', 'recoveryUnavailable', 'geometryUnsupported']),
}).strict();
export const attachmentSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('attached'), delivery: deliveryRefSchema, input: z.null(),
    layout: fixedLayout, recovery: recoverySchema }).strict(), unavailable,
]);
export const nativeAttachmentSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('attached'), delivery: deliveryRefSchema, input: z.null(),
    layout: fixedLayout, recovery: nativeRecoverySchema }).strict(), unavailable,
]);
export type ObserverAttachment = z.infer<typeof nativeAttachmentSchema>;
export type ObserverDelivery = z.infer<typeof deliveryRefSchema>;
export const ackSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('recovery'), recoveryId: observerId, sequence }).strict(),
  z.object({ kind: z.literal('data'), sequence: epoch }).strict(),
]);
export const acknowledgeRequestSchema = z.object({ delivery: deliveryRefSchema, ack: ackSchema }).strict();
export const detachRequestSchema = z.object({ delivery: deliveryRefSchema }).strict();
const data = z.object({ type: z.literal('data'), delivery: deliveryRefSchema, sequence: epoch,
  encoding: z.literal('base64'), data: encodedBytes(16384) }).strict();
const sourceEnded = z.object({ type: z.literal('sourceEnded'), delivery: deliveryRefSchema, lastSequence: sequence }).strict();
const processExited = z.object({ type: z.literal('processExited'), delivery: deliveryRefSchema,
  lastSequence: sequence, exitCode: integer.nullable(), signal: integer.nullable(),
}).strict().refine(value => value.exitCode !== null || value.signal !== null);
const failure = z.object({ type: z.literal('error'), delivery: deliveryRefSchema, error: observerErrorSchema }).strict();
const revoked = z.object({ type: z.literal('revoked'), delivery: deliveryRefSchema,
  reason: z.enum(['approvalRevoked', 'viewDetached', 'sourceUnavailable', 'policyChanged']) }).strict();
const layout = z.object({ type: z.literal('layout'), delivery: deliveryRefSchema, layoutRevision: epoch,
  cols: z.number().int().min(2).max(500), rows: z.number().int().min(2).max(500), recoveryRequired: z.literal(true) }).strict();
export const terminalEventSchema = z.discriminatedUnion('type', [data, sourceEnded, processExited, failure, revoked, layout]);
export const coarseTerminalEventSchema = z.discriminatedUnion('type', [sourceEnded, processExited, failure, revoked, layout]);
const changed = z.object({ type: z.literal('changed'), event: changedEventSchema }).strict();
export const hostEventSchema = z.discriminatedUnion('type', [changed,
  z.object({ type: z.literal('terminal'), event: terminalEventSchema }).strict(),
]);
export const pollSchema = z.object({ events: z.array(hostEventSchema).max(1024) }).strict().refine(value =>
  value.events.reduce((size, item) => size + (item.type === 'terminal' && item.event.type === 'data'
    ? decodedSize(item.event.data) : 0), 0) <= 262144);
export const coarseEventSchema = z.discriminatedUnion('type', [changed,
  z.object({ type: z.literal('terminal'), event: coarseTerminalEventSchema }).strict(),
  z.object({ type: z.literal('reattach'), request: attachRequestSchema }).strict(),
]);
export const coarsePollSchema = reply(z.object({ events: z.array(coarseEventSchema).max(1024) }).strict());
export const pollRequestSchema = z.object({ version: z.literal(2), binding: bindingSchema }).strict();
export type ObserverCoarseEvent = z.infer<typeof coarseEventSchema>;
