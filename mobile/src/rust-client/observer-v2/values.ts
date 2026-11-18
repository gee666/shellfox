import { z } from 'zod';
import { settingsSchema } from '../../../../src/shared/schemas';

// Read validation must not trim proven host facts. Rust validates the trimmed
// name but serializes the original value; keep that distinction on v2 reads.
export function observerEnvironment(stored: boolean) {
  return z.array(z.object({
    name: z.string().refine(value => {
      const name = value.trim();
      return name.length > 0 && name.length <= 256 && !/[=\u0000]/.test(name)
        && (stored || !/[:/\s]/u.test(name));
    }),
    value: z.string().max(32767).refine(value => !value.includes('\u0000') && (stored || !/[\r\n]/.test(value))),
  }).strict()).max(200).refine(values => new Set(values.map(value => value.name.trim())).size === values.length);
}

// V2 never invents read facts from V1's presentation defaults. These leaves are
// optional on the established wire; explicit null and omission stay distinct.
export const observerSettingsSchema = settingsSchema.extend({
  pythonPath: settingsSchema.shape.pythonPath.unwrap().optional(),
  terminalEnv: observerEnvironment(false).optional(),
  autoUpdate: settingsSchema.shape.autoUpdate.unwrap().optional(),
  backgroundColor: settingsSchema.shape.backgroundColor.unwrap().optional(),
  historyPageSize: settingsSchema.shape.historyPageSize.unwrap().optional(),
  sessionSettleHours: settingsSchema.shape.sessionSettleHours.unwrap().optional(),
  sessionDeleteHours: settingsSchema.shape.sessionDeleteHours.unwrap().optional(),
}).strict();
