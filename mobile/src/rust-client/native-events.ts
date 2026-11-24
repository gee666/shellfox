import { z } from 'zod';
import { changedEventSchema, terminalEventSchema, requestSchemas, errorSchema } from '../../../src/shared/schemas';

import { positiveDecimalSchema, unsignedDecimalSchema } from './identifiers';

export const nativeId = positiveDecimalSchema;
const sequence = unsignedDecimalSchema;
export const nativeTerminalStateSchema = z.object({
  terminalHandle: nativeId, state: z.enum(['bound', 'ready', 'recovering', 'closed']),
}).strict();
export type NativeTerminalState = z.infer<typeof nativeTerminalStateSchema>['state'];
export const nativeConnectionSchema = z.object({
  status: z.enum(['disconnected', 'connecting', 'awaitingApproval', 'rejected',
    'connected', 'reconnecting', 'suspended']),
  permission: z.enum(['unknown', 'awaitingApproval', 'approved', 'rejected']),
  connectionId: sequence,
  retryAfterMs: z.number().int().min(0).max(300000).nullable(),
}).strict();
export const nativeOperationSchema = z.object({
  operationId: nativeId, internal: z.boolean(), status: z.number().int().min(0).max(11), result: z.unknown(),
  connection: nativeConnectionSchema,
  resetTerminals: z.array(nativeId).max(32).optional(),
  resetTickets: z.record(nativeId, nativeId)
    .refine((value) => Object.keys(value).length <= 32).optional(),
  resetGeometry: z.record(nativeId, z.object({
    cols: z.number().int().min(1).max(512), rows: z.number().int().min(1).max(512),
  }).strict().refine((grid) => grid.cols * grid.rows <= 65536))
    .refine((value) => Object.keys(value).length <= 32).optional(),
  recoveryReady: z.array(nativeId).max(32).optional(),
  /** Terminals whose input was lost with their attachment; reattach them. */
  reattach: z.array(requestSchemas.attachTerminal).max(32).optional(),
}).strict();
export type NativeOperation = z.infer<typeof nativeOperationSchema>;
export const coarseHostEventSchema = z.union([
  z.object({ type: z.literal('changed'), event: changedEventSchema }).strict(),
  z.object({ type: z.literal('terminal'),
    event: terminalEventSchema.refine((event) => event.type !== 'data', 'Terminal output must stay native.'),
  }).strict(),
  z.object({ type: z.literal('reattach'), request: requestSchemas.attachTerminal }).strict(),
]);
export type CoarseHostEvent = z.infer<typeof coarseHostEventSchema>;
export const coarsePollSchema = z.discriminatedUnion('ok', [
  z.object({ ok: z.literal(true), value: z.object({
    events: z.array(coarseHostEventSchema).max(1024),
  }).strict() }).strict(),
  z.object({ ok: z.literal(false), error: errorSchema }).strict(),
]);
