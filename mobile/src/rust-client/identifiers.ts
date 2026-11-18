import { z } from 'zod';

const MAX_U64 = 18_446_744_073_709_551_615n;
// Zod refinements may run after an earlier regex check failed. Guard conversion
// in the same predicate so safeParse never throws on malformed native IDs.
export const positiveDecimalSchema = z.string().refine((value) =>
  /^[1-9][0-9]{0,19}$/.test(value) && BigInt(value) <= MAX_U64);
export const unsignedDecimalSchema = z.string().refine((value) =>
  /^(0|[1-9][0-9]{0,19})$/.test(value) && BigInt(value) <= MAX_U64);
