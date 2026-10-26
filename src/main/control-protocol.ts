import path from 'node:path';
import { z } from 'zod';
import { sessionSchema } from '../shared/schemas';
import { tabStatSchema } from '../shared/process-stats-schemas';

export const CONTROL_ENDPOINT_FILE = path.join('control', 'endpoint.json');
export const CONTROL_REQUEST_BYTES = 4096;
export const CONTROL_RESPONSE_BYTES = 16 * 1024 * 1024;
export const CONTROL_TIMEOUT_MS = 30_000;
export const controlEndpointPath = (userData: string): string => path.join(userData, CONTROL_ENDPOINT_FILE);

export const controlRequestSchema = z.object({
  action: z.enum(['list', 'stats', 'processes', 'inspect', 'delete', 'settle', 'unsettle', 'tab-open', 'tab-close', 'keep-alive']),
  marker: z.uuid().optional(),
  id: z.string().min(1).max(36).regex(/^[0-9a-f]+(?:-[0-9a-f]*)*$/i).optional(),
  current: z.boolean().optional(),
  confirmed: z.boolean().optional(),
}).strict().superRefine((r, ctx) => {
  const invalid = (message: string) => ctx.addIssue({ code: 'custom', message });
  const targetAction = ['inspect', 'delete', 'settle', 'unsettle'].includes(r.action);
  const tabAction = r.action === 'tab-open' || r.action === 'tab-close';
  const readAction = ['list', 'stats', 'processes'].includes(r.action);
  if (r.id && r.current) invalid('Choose an ID or current context, not both.');
  if (targetAction && !r.id && !r.marker) invalid('A session ID or live terminal context is required.');
  if (tabAction && (r.id !== undefined || !r.marker)) invalid('Tab operations require live terminal context and do not accept an ID.');
  if (!targetAction && !tabAction && !readAction && (r.id !== undefined || r.current !== undefined)) invalid('This action does not accept a target.');
  if (readAction && r.current && !r.marker) invalid('Current selection requires live terminal context.');
  if (r.confirmed !== undefined && r.action !== 'delete') invalid('This action does not accept confirmation.');
});
export const controlEndpointSchema = z.object({ version: z.literal(1), host: z.literal('127.0.0.1'), port: z.number().int().min(1).max(65535), secret: z.string().regex(/^[a-f0-9]{64}$/) }).strict();
export const controlEnvelopeSchema = z.object({ version: z.literal(1), secret: z.string().regex(/^[a-f0-9]{64}$/), request: controlRequestSchema }).strict();
export const controlResponseSchema = z.object({ sessions: z.array(sessionSchema).max(10000).optional(), session: sessionSchema.optional(), tabs: z.array(tabStatSchema).max(64).optional(), message: z.string().max(1000).optional() }).strict();
export const controlReplySchema = z.discriminatedUnion('ok', [
  z.object({ ok: z.literal(true), response: controlResponseSchema }).strict(),
  z.object({ ok: z.literal(false), error: z.string().min(1).max(1000) }).strict(),
]);

/** Exactly one newline-delimited JSON frame per connection. Count bytes before decoding. */
export class ControlFrame {
  private chunks: Buffer[] = [];
  private bytes = 0;
  private done = false;
  constructor(private readonly limit: number) {}
  push(chunk: Buffer): string | undefined {
    if (this.done) throw new Error('Unexpected extra control frame.');
    this.bytes += chunk.length;
    if (this.bytes > this.limit) throw new Error('Control frame is too large.');
    const end = chunk.indexOf(10);
    if (end !== -1 && end !== chunk.length - 1) throw new Error('Unexpected trailing control data.');
    this.chunks.push(chunk);
    if (end === -1) return undefined;
    this.done = true;
    return new TextDecoder('utf-8', { fatal: true }).decode(Buffer.concat(this.chunks).subarray(0, this.bytes - 1));
  }
}
