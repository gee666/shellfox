import { z } from 'zod';
import { idSchema, resultSchema } from './schemas';

export const closeConfirmationSchema = z.object({
  id: idSchema, kind: z.enum(['quit', 'update']), terminalCount: z.number().int().min(0).max(1000000),
}).strict();
export const closeConfirmationStateSchema = closeConfirmationSchema.nullable();
export const closeConfirmationReadSchema = z.object({}).strict();
export const closeConfirmationResponseSchema = z.object({ id: idSchema, approved: z.boolean() }).strict();
export const closeConfirmationReadResultSchema = resultSchema(closeConfirmationStateSchema);
export const closeConfirmationResponseResultSchema = resultSchema(z.object({ accepted: z.literal(true) }).strict());
