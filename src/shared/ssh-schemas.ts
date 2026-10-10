import { z } from 'zod';
import { normalizeSshInput } from './ssh-endpoint';
const text=(max:number)=>z.string().max(max).refine(s=>!/[\x00-\x1f\x7f]/.test(s),'Control characters are not allowed');
export const sshProfileObjectSchema=z.object({
 id:z.string().uuid(),name:text(200).trim().min(1),host:text(1000).min(1).refine(s=>!/@|\s/u.test(s),'Host must not contain @ or whitespace'),
 port:z.number().int().min(1).max(65535),user:text(200),password:z.string().max(65536).nullable().optional(),keyFile:text(32767).nullable(),remoteCwd:text(32767).nullable(),
}).strict();
export const sshProfileInputSchema=z.preprocess(normalizeSshInput,sshProfileObjectSchema);
export const sshConnectionOptionsSchema=z.object({
 addressFamily:z.union([z.literal(0),z.literal(4),z.literal(6)]).optional(),tryAgent:z.boolean().optional(),agentForward:z.boolean().optional(),
 unsupportedProxy:z.object({method:text(100),host:text(1000).nullable(),port:z.number().int().min(0).max(65535).nullable()}).strict().optional(),
}).strict();
export const sshStoredProfileSchema=z.preprocess(normalizeSshInput,sshProfileObjectSchema.extend({password:z.string().max(65536).nullable(),source:z.enum(['manual','putty']),connectionOptions:sshConnectionOptionsSchema.optional()}));
export const sshProfileDtoSchema=sshProfileObjectSchema.omit({password:true}).extend({source:z.enum(['manual','putty']),hasPassword:z.boolean()});
export const sshFileSchema=z.object({version:z.literal(1),profiles:z.array(sshStoredProfileSchema).max(10000)}).strict().refine(f=>new Set(f.profiles.map(p=>p.id)).size===f.profiles.length&&new Set(f.profiles.map(p=>p.name.toLowerCase())).size===f.profiles.length,'Duplicate SSH profiles');
