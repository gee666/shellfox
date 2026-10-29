import { z } from 'zod';

const id = z.uuid();
const text = z.string().max(32768).refine(value => !value.includes('\0'), 'NUL is not allowed.');
const base64 = z.string().max(65536).regex(/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/);
export const automationStartSchema = z.object({
  requestId: id, executable: text.min(1), args: z.array(text).max(4096), cwd: text.min(1),
  env: z.record(z.string().min(1).max(1024).regex(/^[^=\0]+$/), text),
  title: z.string().min(1).max(200).optional(), detached: z.boolean(),
}).strict();
export const automationRunSchema = z.object({
  id, pid: z.number().int().nonnegative(), executable: text, cwd: text, title: z.string().min(1).max(200),
  state: z.enum(['running', 'exited']), exitCode: z.number().int().nullable(), signal: z.string().nullable(),
  sessionId: id.nullable(), tabId: id.nullable(),
}).strict();
const identity = z.object({ id }).strict();
export const automationRequestSchemas = {
  'automation.start': automationStartSchema,
  'automation.list': z.object({}).strict(),
  'automation.read': z.object({ id, after: z.number().int().nonnegative() }).strict(),
  'automation.write': z.object({ id, data: base64, eof: z.boolean() }).strict(),
  'automation.stop': identity,
  'automation.show': identity,
};
export const automationResponseSchemas = {
  'automation.start': automationRunSchema,
  'automation.list': z.array(automationRunSchema).max(64),
  'automation.read': z.object({
    frames: z.array(z.object({ sequence: z.number().int().positive(), stream: z.enum(['stdout', 'stderr']), data: base64 }).strict()).max(32),
    run: automationRunSchema,
  }).strict(),
  'automation.write': z.object({ accepted: z.literal(true) }).strict(),
  'automation.stop': z.object({ stopped: z.literal(true) }).strict(),
  'automation.show': automationRunSchema,
};
export type AutomationStart = z.infer<typeof automationStartSchema>;
export type AutomationRun = z.infer<typeof automationRunSchema>;
export type AutomationFrame = z.infer<(typeof automationResponseSchemas)['automation.read']>['frames'][number];
export type AutomationMethod = keyof typeof automationRequestSchemas;
export function isAutomationMethod(method: string): method is AutomationMethod { return Object.hasOwn(automationRequestSchemas, method); }
