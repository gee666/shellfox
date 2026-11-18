import { z } from 'zod';
import { errorSchema, timestampSchema } from '../../../../src/shared/schemas';

export const VERSION = 2;
export const PROFILE = 'observer-v2';
export const observerId = z.uuid().refine(value => value !== '00000000-0000-0000-0000-000000000000');
export const sequence = z.number().int().min(0).max(Number.MAX_SAFE_INTEGER);
export const epoch = sequence.positive();
export const integer = z.number().int().min(-Number.MAX_SAFE_INTEGER).max(Number.MAX_SAFE_INTEGER);
export const count = z.number().int().min(0).max(1_000_000);
export const boundedText = (max: number) => z.string().max(max).refine(value => !/[\x00-\x1f\x7f]/.test(value));
export const timestamp = timestampSchema;

export function fact<T extends z.ZodType>(value: T) {
  // Validate presence before a leaf schema can apply a backwards-compatible default.
  const known = z.object({ state: z.literal('known'), value: z.unknown() }).strict()
    .transform((input, ctx) => {
      if (!Object.hasOwn(input, 'value') || input.value === undefined) {
        ctx.addIssue({ code: 'custom', message: 'Known facts require an explicit value.', path: ['value'] });
        return z.NEVER;
      }
      const parsed = value.safeParse(input.value);
      if (!parsed.success) {
        ctx.addIssue({ code: 'custom', message: 'Invalid known fact value.', path: ['value'] });
        return z.NEVER;
      }
      return { state: input.state, value: parsed.data };
    });
  return z.union([known,
    z.object({ state: z.literal('unknown'), reason: z.enum(['notRecorded', 'pending', 'unavailable', 'invalidEvidence']) }).strict(),
    z.object({ state: z.literal('notApplicable') }).strict(),
  ]);
}
export const projectionIssueSchema = z.object({
  reason: z.enum(['invalidMetadata', 'inconsistentMembership', 'evidenceUnavailable', 'unsupportedSource']),
  fields: z.array(z.enum(['sourceKind', 'adapterId', 'shellId', 'profileId', 'createdAt', 'cwd', 'status', 'runtime', 'geometry', 'recovery'])).min(1).max(10),
}).strict();
const detail = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('application') }).strict(),
  z.object({ kind: z.literal('profileIncompatible') }).strict(),
  z.object({ kind: z.literal('coverageUnsupported'), issues: z.array(projectionIssueSchema).min(1).max(64), issueCount: z.number().int().min(1).max(4294967295) }).strict()
    .refine(value => value.issueCount >= value.issues.length),
  z.object({ kind: z.literal('resourceLimit'), limit: z.enum(['requestBytes', 'responseBytes', 'recoveryBytes', 'inventory', 'events', 'pendingOperations']) }).strict(),
]);
export const observerErrorSchema = errorSchema.extend({ detail }).strict().refine(error => {
  switch (error.detail.kind) {
    case 'profileIncompatible': case 'coverageUnsupported': return error.code === 'UNSUPPORTED' && !error.retryable;
    case 'resourceLimit': return (error.code === 'UNSUPPORTED' || error.code === 'VALIDATION') && !error.retryable;
    case 'application': return true;
  }
});
export type ObserverError = z.infer<typeof observerErrorSchema>;
export const bindingSchema = z.object({
  clientId: observerId, ownerId: observerId, connectionId: observerId, profile: z.literal(PROFILE),
}).strict();
export type ObserverBinding = z.infer<typeof bindingSchema>;
export function sameBinding(left: ObserverBinding, right: ObserverBinding): boolean {
  return left.clientId === right.clientId && left.ownerId === right.ownerId
    && left.connectionId === right.connectionId && left.profile === right.profile;
}
export function reply<T extends z.ZodType>(value: T) {
  return z.discriminatedUnion('ok', [
    z.object({ version: z.literal(VERSION), ok: z.literal(true), value }).strict(),
    z.object({ version: z.literal(VERSION), ok: z.literal(false), error: observerErrorSchema }).strict(),
  ]);
}
export function boundReply<T extends z.ZodType>(value: T) {
  return z.discriminatedUnion('ok', [
    z.object({ version: z.literal(VERSION), binding: bindingSchema, ok: z.literal(true), value }).strict(),
    z.object({ version: z.literal(VERSION), binding: bindingSchema, ok: z.literal(false), error: observerErrorSchema }).strict(),
  ]);
}

const identifiers = z.array(z.string().min(1).max(64).regex(/^[a-zA-Z0-9-]+$/)).max(16)
  .refine(values => new Set(values).size === values.length);
export const discoverRequestSchema = z.object({}).strict();
export const discoverySchema = z.object({
  service: z.literal('shellfox'), ownerId: observerId, profiles: identifiers,
  catalogModes: identifiers, deliveryFormats: identifiers, recoveryFormats: identifiers,
}).strict();
const selection = {
  profile: z.literal(PROFILE), catalogMode: z.literal('complete'),
  deliveryFormat: z.literal('vt-bytes-base64-v1'), recoveryFormat: z.literal('formatted-active-buffer-v1'),
};
export const pairOfferSchema = z.object({ version: z.literal(VERSION), clientId: observerId,
  attemptId: observerId, ownerId: observerId, ...selection }).strict();
export const pairSelectionSchema = z.object({ clientId: observerId, attemptId: observerId,
  ownerId: observerId, connectionId: observerId, ...selection }).strict();
export const disconnectSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('attempt'), version: z.literal(VERSION), clientId: observerId, attemptId: observerId }).strict(),
  z.object({ kind: z.literal('connection'), version: z.literal(VERSION), binding: bindingSchema }).strict(),
]);
export const disconnectedSchema = z.object({ disconnected: z.literal(true) }).strict();
function supportsObserver(value: z.infer<typeof discoverySchema>): boolean {
  return value.profiles.includes(PROFILE) && value.catalogModes.includes('complete')
    && value.deliveryFormats.includes('vt-bytes-base64-v1') && value.recoveryFormats.includes('formatted-active-buffer-v1');
}
export const discoveryOutcomeSchema = z.union([
  z.object({ kind: z.literal('available'), discovery: discoverySchema }).strict(),
  z.object({ kind: z.literal('incompatible'), discovery: discoverySchema }).strict(),
  z.object({ kind: z.literal('legacyAvailable') }).strict(),
  z.object({ version: z.literal(VERSION), ok: z.literal(false), error: observerErrorSchema }).strict(),
]).refine(value => !('kind' in value) || value.kind === 'legacyAvailable'
  || supportsObserver(value.discovery) === (value.kind === 'available'));
