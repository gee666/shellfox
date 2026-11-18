import { z } from 'zod';
import { positiveDecimalSchema as id } from '../identifiers';
import { bindingSchema, PROFILE } from './primitives';
import type { NativeTerminalBinding } from '../adapter';

export interface ObserverNativeBinding extends Pick<NativeTerminalBinding,
  'abiVersion' | 'createTerminal' | 'configureTerminal' | 'closeTerminal'> {
  observerV2AbiVersion(): number;
  clientProfile(clientHandle: string): Promise<string>;
  createObserverV2Client(peerId: string, clientId: string): Promise<string>;
  discoverObserverV2(clientHandle: string, offerLegacy: boolean): Promise<string>;
  pairObserverV2(clientHandle: string, attemptId: string): Promise<string>;
  observerV2Operation(clientHandle: string, json: string): Promise<string>;
  pollObserverV2(clientHandle: string): Promise<string>;
  reconnectObserverV2(clientHandle: string, clientId: string): Promise<void>;
  disconnectObserverV2(clientHandle: string): Promise<string>;
  cancel(clientHandle: string, operationId: string): Promise<void>;
  bindTerminal(clientHandle: string, tabId: string, terminalHandle: string): Promise<void>;
  unbindTerminal(clientHandle: string, terminalHandle: string): Promise<void>;
  readEvent(clientHandle: string): Promise<string | null>;
  closeClient(clientHandle: string): Promise<void>;
}
// API shape, never a current server input/layout grant.
export const clientProfileSchema = z.discriminatedUnion('profile', [
  z.object({ profile: z.literal(1), apiCapabilities: z.literal(7) }).strict(),
  z.object({ profile: z.literal(2), apiCapabilities: z.literal(1) }).strict(),
]);
const grid = z.object({ cols: z.number().int().min(1).max(512), rows: z.number().int().min(1).max(512) })
  .strict().refine(value => value.cols * value.rows <= 65536);
export const nativeObserverEventSchema = z.object({
  profile: z.literal(PROFILE), operationId: id, internal: z.boolean(),
  status: z.number().int().min(0).max(11), result: z.unknown(),
  connection: z.object({ status: z.enum(['disconnected', 'discovered', 'connecting', 'connected',
    'legacyAvailable', 'awaitingApproval', 'rejected', 'reconnecting']), binding: bindingSchema.nullable() }).strict(),
  resetTerminals: z.array(id).max(32).optional(),
  resetTickets: z.record(id, id).refine(value => Object.keys(value).length <= 32).optional(),
  resetGeometry: z.record(id, grid).refine(value => Object.keys(value).length <= 32).optional(),
  recoveryReady: z.array(id).max(32).optional(),
}).strict();
export type NativeObserverEvent = z.infer<typeof nativeObserverEventSchema>;
