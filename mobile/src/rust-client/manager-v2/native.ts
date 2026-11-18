import { z } from 'zod';
import type { NativeRemoteBinding } from '../remote-adapter';
import type { NativeTerminalBinding } from '../adapter';
import { positiveDecimalSchema } from '../identifiers';
import { bindingSchema } from './schemas';
export interface ManagerNativeBinding extends NativeRemoteBinding, NativeTerminalBinding {
  managerV2AbiVersion(): number;
  clientProfile(client:string):Promise<string>;
  createManagerV2Client(peer:string,identity:string):Promise<string>;
  discoverManagerV2(client:string):Promise<string>;
  pairManagerV2(client:string,attempt:string):Promise<string>;
  managerV2Operation(client:string,json:string):Promise<string>;
  pollManagerV2(client:string):Promise<string>;
  reconnectManagerV2(client:string,identity:string):Promise<void>;
  disconnectManagerV2(client:string):Promise<string>;
}
export const profileSchema=z.object({profile:z.literal(3),apiCapabilities:z.literal(7)}).strict();
export const nativeManagerEventSchema=z.object({
  profile:z.literal('manager-v2'),operationId:positiveDecimalSchema,internal:z.boolean(),status:z.number().int().min(0).max(11),result:z.unknown(),
  connection:z.object({status:z.enum(['disconnected','discovered','connecting','connected','legacyAvailable','awaitingApproval','rejected','reconnecting']),binding:bindingSchema.nullable()}).strict(),
  resetTerminals:z.array(positiveDecimalSchema).max(32).optional(),resetTickets:z.record(positiveDecimalSchema,positiveDecimalSchema).optional(),resetGeometry:z.record(positiveDecimalSchema,z.object({cols:z.number().int().positive(),rows:z.number().int().positive()}).strict()).optional(),recoveryReady:z.array(positiveDecimalSchema).max(32).optional(),retiredTerminals:z.array(positiveDecimalSchema).max(32).optional(),
}).strict().refine(event=>Object.hasOwn(event,'result')&&(event.resetTerminals?.length??0)===0&&Object.keys(event.resetTickets??{}).length===0&&Object.keys(event.resetGeometry??{}).length===0);
export type NativeManagerEvent=z.infer<typeof nativeManagerEventSchema>;
