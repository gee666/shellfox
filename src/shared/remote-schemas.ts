import { z } from 'zod';
import { changedEventSchema, terminalEventSchema, requestSchemas, responseSchemas, resultSchema, envVarsSchemaFor, idSchema, errorSchema } from './schemas';

// Encoded JSON must fit both loopback and tailnet request readers.
export const REMOTE_REQUEST_BYTES = 1024 * 1024;
export const remoteMethods = ['getSnapshot', 'getSessionStats', 'getTerminalProfiles', 'attachTerminal', 'writeTerminal', 'resizeTerminal', 'closeTab', 'acknowledgeTerminal', 'detachTerminal', 'createSession', 'addTab', 'activateSession', 'renameSession', 'renameTab', 'reorderTabs', 'setSessionPinned', 'settleSession', 'unsettleSession', 'deleteSession', 'clearSessionError', 'getHomeDirectory', 'resolveDirectory', 'createDirectory', 'completeDirectory', 'setSessionEnv', 'setVisibleSessions'] as const;
export const remoteMethodSchema = z.enum(remoteMethods);
export const remotePeerIdSchema = z.string().min(1).max(200).regex(/^[a-zA-Z0-9_.:-]+$/);
export const remoteHostnameSchema = z.string().min(1).max(63).regex(/^[a-zA-Z0-9](?:[a-zA-Z0-9-]*[a-zA-Z0-9])?$/);
const label = z.string().min(1).max(253).refine(s => !/[\x00-\x1f\x7f]/.test(s));
export const remoteNetworkPeerSchema = z.object({ id: remotePeerIdSchema, hostname: label, addresses: z.array(z.string().min(1).max(100)).max(16), online: z.boolean() }).strict();
export const remotePeerSchema = remoteNetworkPeerSchema.extend({ connection: z.enum(['disconnected', 'connecting', 'awaiting-approval', 'rejected', 'connected', 'reconnecting']), lastError: errorSchema.nullable().optional(), approved: z.boolean(), remembered: z.boolean() });
export const remoteApprovalSchema = z.object({ peerId: remotePeerIdSchema, hostname: label, approvedAt: z.iso.datetime() }).strict();
export const remoteStatusSchema = z.object({ status: z.enum(['disabled', 'starting', 'login-required', 'online', 'error']), loginUrl: z.url().max(2048).nullable().refine(s => s === null || s.startsWith('https://')), error: z.string().max(1000).nullable() }).strict();
export const remoteStateSchema = remoteStatusSchema.extend({ enabled: z.boolean(), hostname: remoteHostnameSchema, peers: z.array(remotePeerSchema).max(1000), approvals: z.array(remoteApprovalSchema).max(1000), pending: z.array(remoteNetworkPeerSchema).max(100) });
export const remoteEventSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('state'), state: remoteStateSchema }).strict(),
  z.object({ type: z.literal('changed'), peerId: remotePeerIdSchema, event: changedEventSchema }).strict(),
  z.object({ type: z.literal('terminal'), peerId: remotePeerIdSchema, event: terminalEventSchema }).strict(),
]);
export const remoteHostEventSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('changed'), event: changedEventSchema }).strict(),
  z.object({ type: z.literal('terminal'), event: terminalEventSchema }).strict(),
]);
const peer = z.object({ peerId: remotePeerIdSchema }).strict();
const empty = z.object({}).strict();
// The sender may run a different OS. The receiving host applies its own write policy.
export const remotePayloadSchemas = { ...requestSchemas, setSessionEnv: z.object({ sessionId: idSchema, env: envVarsSchemaFor(false) }).strict() };
export const remoteForwardRequestSchema = z.object({ peerId: remotePeerIdSchema, method: remoteMethodSchema, payload: z.unknown() }).strict().superRefine((input, ctx) => {
  if (!remotePayloadSchemas[input.method].safeParse(input.payload).success) {
    ctx.addIssue({ code: 'custom', message: 'Invalid remote request payload' }); return;
  }
  // Reserve space for the client ID, proxy routing fields, and protocol envelope.
  // Measure JSON bytes, not raw terminal bytes: control characters expand sixfold.
  try {
    if (new TextEncoder().encode(JSON.stringify(input)).byteLength > REMOTE_REQUEST_BYTES - 1024) ctx.addIssue({ code: 'custom', message: 'Remote request exceeds the encoded size limit' });
  } catch { ctx.addIssue({ code: 'custom', message: 'Remote request is not JSON encodable' }); }
});
export const remoteRequestSchemas = { getState: empty, configure: z.object({ enabled: z.boolean(), hostname: remoteHostnameSchema }).strict(), login: z.object({ authKey: z.string().min(1).max(4096).optional() }).strict(), connect: peer, disconnect: peer, approve: peer, reject: peer, revoke: peer, forget: peer, request: remoteForwardRequestSchema };
export const remoteResponseSchemas = { getState: resultSchema(remoteStateSchema), configure: resultSchema(remoteStateSchema), login: resultSchema(remoteStateSchema), connect: resultSchema(remoteStateSchema), disconnect: resultSchema(remoteStateSchema), approve: resultSchema(remoteStateSchema), reject: resultSchema(remoteStateSchema), revoke: resultSchema(remoteStateSchema), forget: resultSchema(remoteStateSchema) };
export const remoteEnvelopeSchema = z.object({ method: z.enum(['getState', 'configure', 'login', 'connect', 'disconnect', 'approve', 'reject', 'revoke', 'forget', 'request']), payload: z.unknown() }).strict();
export const remotePairResponseSchema = resultSchema(z.object({ paired: z.literal(true) }).strict());
export const remotePollResponseSchema = resultSchema(z.object({ events: z.array(remoteHostEventSchema).max(1024) }).strict());
export { requestSchemas, responseSchemas };
