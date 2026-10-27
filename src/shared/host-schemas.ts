import { z } from 'zod';
import { terminalResponseCompatibility } from './terminal-compat';
import { changedEventSchema, keepAwakeStateSchema, requestSchemas, responseSchemas } from './schemas';
import { terminalEventSchema } from './schemas';
import { remoteEventSchema, remoteRequestSchemas, remoteResponseSchemas } from './remote-schemas';

export const HOST_REQUEST_BYTES = 1024 * 1024;
export const HOST_RESPONSE_BYTES = 16 * 1024 * 1024;
export const HOST_STOP_TIMEOUT_MS = 120_000;
export const hostStatusSchema = z.object({
  mode: z.enum(['desktop', 'headless']), pid: z.number().int().positive(), startedAt: z.iso.datetime(),
  capabilities: z.object({ keepAwake: z.boolean(), desktopActions: z.boolean(), administrator: z.boolean() }).strict(),
}).strict();
export const hostEndpointSchema = z.object({ version: z.literal(1), host: z.literal('127.0.0.1'), port: z.number().int().min(1).max(65535), secret: z.string().regex(/^[a-f0-9]{64}$/), mode: z.enum(['desktop', 'headless']) }).strict();
export const hostEventSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('changed'), event: changedEventSchema }).strict(),
  z.object({ type: z.literal('terminal'), event: terminalEventSchema }).strict(),
  z.object({ type: z.literal('remote'), event: remoteEventSchema }).strict(),
  z.object({ type: z.literal('keepAwake'), event: keepAwakeStateSchema }).strict(),
]);
const empty = z.object({}).strict();
export const hostRequestSchemas = {
  ...requestSchemas,
  'remote.getState': remoteRequestSchemas.getState,
  'remote.configure': remoteRequestSchemas.configure,
  'remote.login': remoteRequestSchemas.login,
  'remote.connect': remoteRequestSchemas.connect,
  'remote.disconnect': remoteRequestSchemas.disconnect,
  'remote.approve': remoteRequestSchemas.approve,
  'remote.reject': remoteRequestSchemas.reject,
  'remote.revoke': remoteRequestSchemas.revoke,
  'remote.forget': remoteRequestSchemas.forget,
  'remote.request': remoteRequestSchemas.request,
  'host.status': empty, 'host.stop': z.object({ wait: z.boolean().optional() }).strict(), 'host.open': empty, 'host.close': empty, 'host.poll': empty,
} as const;
export const hostResponseSchemas = {
  ...responseSchemas,
  'remote.getState': remoteResponseSchemas.getState,
  'remote.configure': remoteResponseSchemas.configure,
  'remote.login': remoteResponseSchemas.login,
  'remote.connect': remoteResponseSchemas.connect,
  'remote.disconnect': remoteResponseSchemas.disconnect,
  'remote.approve': remoteResponseSchemas.approve,
  'remote.reject': remoteResponseSchemas.reject,
  'remote.revoke': remoteResponseSchemas.revoke,
  'remote.forget': remoteResponseSchemas.forget,
  // The forwarded method selects its result schema at both ends of the connection.
  'remote.request': z.unknown(),
  'host.status': hostStatusSchema,
  'host.stop': z.union([z.object({ stopping: z.literal(true) }).strict(), z.object({ stopped: z.literal(true) }).strict()]),
  'host.open': z.object({ clientId: z.uuid() }).strict(),
  'host.close': z.object({ closed: z.literal(true) }).strict(),
  'host.poll': z.object({ events: z.array(hostEventSchema).max(1024) }).strict(),
} as const;
export const hostEnvelopeSchema = z.object({ version: z.literal(1), requestId: z.uuid(), clientId: z.uuid().optional(), method: z.enum(Object.keys(hostRequestSchemas) as [keyof typeof hostRequestSchemas, ...Array<keyof typeof hostRequestSchemas>]), payload: z.unknown() }).strict();
export const hostReplySchema = z.discriminatedUnion('ok', [
  z.object({ requestId: z.uuid(), ok: z.literal(true), value: z.unknown() }).strict(),
  z.object({ requestId: z.uuid(), ok: z.literal(false), error: z.string().min(1).max(1000) }).strict(),
]);
export function parseHostResponse<M extends keyof typeof hostResponseSchemas>(method: M, raw: unknown): z.output<(typeof hostResponseSchemas)[M]>;
export function parseHostResponse(method: keyof typeof hostResponseSchemas, raw: unknown): unknown {
  return hostResponseSchemas[method].parse(terminalResponseCompatibility(method, raw));
}
