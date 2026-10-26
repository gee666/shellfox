import { z } from 'zod';

const measurement = z.number().finite().nonnegative().nullable();
export const processStatSchema = z.object({ pid: z.number().int().positive().max(4294967295), parentPid: z.number().int().nonnegative().max(4294967295), name: z.string().max(4096), memoryBytes: z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER).nullable(), elapsedSeconds: measurement, domain: z.string().min(1).max(4096) }).strict();
export const tabStatSchema = z.object({ tabId: z.uuid(), processes: z.array(processStatSchema).max(100000), memoryBytes: z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER).nullable(), elapsedSeconds: measurement, reason: z.string().max(65536).optional() }).strict();
export type ProcessStatDto = z.infer<typeof processStatSchema>;
export type TabStatDto = z.infer<typeof tabStatSchema>;
