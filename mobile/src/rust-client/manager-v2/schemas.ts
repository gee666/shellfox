import { z } from 'zod';
import { errorSchema, changedEventSchema } from '../../../../src/shared/schemas';
import { observerId as id, sequence, epoch, projectionIssueSchema, discoverySchema } from '../observer-v2/primitives';
export { snapshotSchema, catalogEntrySchema, sessionStatsSchema } from '../observer-v2/catalog';
export const PROFILE = 'manager-v2';
const detailSchema=z.discriminatedUnion('kind',[
 z.object({kind:z.literal('application')}).strict(),z.object({kind:z.literal('profileIncompatible')}).strict(),
 z.object({kind:z.literal('coverageUnsupported'),issues:z.array(projectionIssueSchema).min(1).max(64),issueCount:z.number().int().positive().max(4294967295)}).strict(),
 z.object({kind:z.literal('resourceLimit'),limit:z.enum(['requestBytes','responseBytes','recoveryBytes','inventory','events','pendingOperations','inputPayloadBytes','encodedInputBytes'])}).strict(),
 z.object({kind:z.literal('staleLayout'),recoveryRequired:z.literal(true)}).strict(),
 z.object({kind:z.literal('inputOutcomeUnknown'),inputId:id,retryInput:z.literal(false)}).strict(),
]);
export const managerErrorSchema=errorSchema.extend({detail:detailSchema}).strict().refine(v=>!['staleLayout','inputOutcomeUnknown','profileIncompatible','coverageUnsupported','resourceLimit'].includes(v.detail.kind)||!v.retryable);
export type ManagerError=z.infer<typeof managerErrorSchema>;
export function reply<T extends z.ZodType>(value:T){return z.discriminatedUnion('ok',[z.object({version:z.literal(2),ok:z.literal(true),value}).strict(),z.object({version:z.literal(2),ok:z.literal(false),error:managerErrorSchema}).strict()]);}
export const bindingSchema = z.object({ clientId:id, ownerId:id, connectionId:id, profile:z.literal(PROFILE) }).strict();
export type ManagerBinding = z.infer<typeof bindingSchema>;
export function sameBinding(a:ManagerBinding,b:ManagerBinding):boolean{return a.clientId===b.clientId&&a.ownerId===b.ownerId&&a.connectionId===b.connectionId&&a.profile===b.profile;}
const limits = z.object({ maxEncodedBytes:sequence.max(4194304), maxAllocationBytes:sequence, maxWorkUnits:sequence,
  maxCells:sequence.max(1048576), maxHistoryRows:sequence.max(10000), maxContinuationBytes:sequence.max(65536),
  maxColumns:sequence.max(4096),maxRows:sequence.max(4096),maxPixelWidth:sequence.max(65536),maxPixelHeight:sequence.max(65536),maxPageRows:sequence.max(65535),maxPages:sequence.max(1000),maxRecords:sequence.max(10000), }).strict();
export const codecSchema = z.object({engineIdentity:z.string().regex(/^[0-9a-f]{64}$/),extensionAbi:z.literal(1),terminalProfileAbi:z.literal(1),snapshotBudgetAbi:z.literal(1),wireProfile:z.literal(1),libraryVersion:z.literal(65537),snapshotVersion:z.literal(1),terminalProfile:z.literal(0)}).strict();
export const pairSelectionSchema = z.object({clientId:id,attemptId:id,ownerId:id,connectionId:id,profile:z.literal(PROFILE),catalogMode:z.literal('complete'),deliveryFormat:z.literal('vt-bytes-base64-v1'),recoveryFormat:z.literal('native0-raw-v1'),inputFormat:z.literal('manager-input-v1'),devicePolicy:z.literal('control-full-account-v1'),devicePermission:z.literal('write'),native0Sender:codecSchema,native0Receiver:codecSchema}).strict();
export type PairSelection=z.infer<typeof pairSelectionSchema>;
export const discoveryOutcomeSchema=z.union([z.object({kind:z.literal('available'),discovery:discoverySchema}).strict(),z.object({kind:z.literal('incompatible'),discovery:discoverySchema}).strict(),z.object({version:z.literal(2),ok:z.literal(false),error:managerErrorSchema}).strict()]).refine(value=>!('kind' in value)||(value.kind==='available')===(value.discovery.profiles.includes(PROFILE)&&value.discovery.catalogModes.includes('complete')&&value.discovery.deliveryFormats.includes('vt-bytes-base64-v1')&&value.discovery.recoveryFormats.includes('native0-raw-v1')));
export const deliverySchema=z.object({tabId:id,generation:id,deliveryId:id,deliveryEpoch:epoch}).strict();
export const controlSchema=z.object({tabId:id,generation:id,controlId:id,controlEpoch:epoch}).strict();
export const layoutSchema=z.object({tabId:id,generation:id,layoutId:id,layoutEpoch:epoch}).strict();
export const nativeRecoverySchema=z.object({format:z.literal('native0-raw-v1'),recoveryId:id,encoding:z.literal('base64'),lastSequence:sequence,cols:z.number().int().positive().max(4096),rows:z.number().int().positive().max(4096),layoutRevision:epoch,metadata:z.object({codec:codecSchema,state:z.object({abiVersion:z.literal(1),terminalProfile:z.literal(0),precedingJoin:z.union([z.literal(0),z.literal(1)])}).strict(),configuration:z.object({runtimeImageStorageLimit:sequence,runtimeResizePullScrollback:z.boolean()}).strict(),limits}).strict()}).strict();
export const attachmentSchema=z.discriminatedUnion('kind',[
 z.object({kind:z.literal('attached'),delivery:deliverySchema,input:controlSchema.nullable(),inputState:z.enum(['owned','busy','notReady','intrinsicReadOnly','sourcePolicyDenied']),layout:layoutSchema.nullable(),recovery:nativeRecoverySchema}).strict().refine(v=>(v.inputState==='owned')===(v.input!==null)&&(!v.input||(v.input.tabId===v.delivery.tabId&&v.input.generation===v.delivery.generation))&&(!v.layout||(v.layout.tabId===v.delivery.tabId&&v.layout.generation===v.delivery.generation))&&v.recovery.cols*v.recovery.rows<=1048576),
 z.object({kind:z.literal('unavailable'),tabId:id,reason:z.enum(['sourceUnavailable','factsUnavailable','recoveryUnavailable','geometryUnsupported'])}).strict(),
]);
export type ManagerAttachment=z.infer<typeof attachmentSchema>;
export const attachSchema=z.object({tabId:id,resume:z.object({generation:id,afterSequence:sequence}).strict().nullable(),control:z.enum(['none','ifFree'])}).strict();
export const takeSchema=z.object({delivery:deliverySchema,mode:z.literal('takeover')}).strict();
export const detachSchema=z.object({delivery:deliverySchema,releaseControl:controlSchema.nullable()}).strict();
export const terminalEventSchema=z.discriminatedUnion('type',[
 z.object({type:z.literal('sourceEnded'),delivery:deliverySchema,lastSequence:sequence}).strict(),
 z.object({type:z.literal('processExited'),delivery:deliverySchema,lastSequence:sequence,exitCode:z.number().int().nullable(),signal:z.number().int().nullable()}).strict(),
 z.object({type:z.literal('revoked'),delivery:deliverySchema,reason:z.enum(['approvalRevoked','viewDetached','sourceUnavailable','policyChanged','controlLost','controlReleased','generationChanged','permissionChanged'])}).strict(),
 z.object({type:z.literal('layout'),delivery:deliverySchema,layoutRevision:epoch,cols:z.number().int().positive().max(32767),rows:z.number().int().positive().max(32767),recoveryRequired:z.boolean(),lastSequence:sequence}).strict(),
 z.object({type:z.literal('error'),delivery:deliverySchema,error:managerErrorSchema}).strict(),
]);
export type ManagerTerminalEvent=z.infer<typeof terminalEventSchema>;
export const changedSchema=z.object({type:z.literal('changed'),event:changedEventSchema}).strict();
export const coarseHostEventSchema=z.union([changedSchema,z.object({type:z.literal('terminal'),event:terminalEventSchema}).strict()]);
export const pollSchema=reply(z.object({events:z.array(coarseHostEventSchema).max(1024)}).strict());
export type ManagerHostEvent=z.infer<typeof coarseHostEventSchema>;
