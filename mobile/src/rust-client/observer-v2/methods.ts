import { z } from 'zod';
import { remoteMethods, remotePayloadSchemas, responseSchemas } from '../../../../src/shared/remote-schemas';
import { bindingSchema, reply, type ObserverError } from './primitives';
import { snapshotSchema, catalogEntrySchema, sessionStatsSchema } from './catalog';
import { observerSettingsSchema } from './values';
import { attachmentSchema, attachRequestSchema, acknowledgeRequestSchema, detachRequestSchema } from './delivery';

export const observerMethods = remoteMethods;
export type ObserverMethod = typeof observerMethods[number];
export const deniedMethods = ['takeTerminalControl', 'writeTerminal', 'resizeTerminal',
  'beginTerminalUpload', 'writeTerminalUpload', 'finishTerminalUpload', 'cancelTerminalUpload'] as const;
export type DeniedObserverMethod = typeof deniedMethods[number];
// Recovery and ACK data belong to native bindings, never generic presentation RPC.
export type ObserverManagerMethod = Exclude<ObserverMethod, DeniedObserverMethod | 'attachTerminal' | 'acknowledgeTerminal' | 'detachTerminal'>;

export const observerRequestSchemas = { ...remotePayloadSchemas,
  attachTerminal: attachRequestSchema, acknowledgeTerminal: acknowledgeRequestSchema, detachTerminal: detachRequestSchema,
};
export const observerValueSchemas = {
  getSnapshot: snapshotSchema,
  getSessionStats: sessionStatsSchema,
  createSession: catalogEntrySchema,
  addTab: catalogEntrySchema,
  activateSession: catalogEntrySchema,
  renameSession: catalogEntrySchema,
  renameTab: catalogEntrySchema,
  reorderTabs: catalogEntrySchema,
  setSessionPinned: catalogEntrySchema,
  settleSession: catalogEntrySchema,
  unsettleSession: catalogEntrySchema,
  clearSessionError: catalogEntrySchema,
  setSessionEnv: catalogEntrySchema,
  closeTab: catalogEntrySchema,
  attachTerminal: attachmentSchema,
  acknowledgeTerminal: z.object({ acknowledged: z.literal(true) }).strict(),
  detachTerminal: z.object({ detached: z.literal(true) }).strict(),
  getTerminalProfiles: responseSchemas.getTerminalProfiles.options[0].shape.value,
  getHomeDirectory: responseSchemas.getHomeDirectory.options[0].shape.value,
  deleteSession: responseSchemas.deleteSession.options[0].shape.value,
  setTerminalEnv: observerSettingsSchema,
  setVisibleSessions: responseSchemas.setVisibleSessions.options[0].shape.value,
  takeTerminalControl: responseSchemas.takeTerminalControl.options[0].shape.value,
  writeTerminal: responseSchemas.writeTerminal.options[0].shape.value,
  resizeTerminal: responseSchemas.resizeTerminal.options[0].shape.value,
  listFiles: responseSchemas.listFiles.options[0].shape.value,
  readFile: responseSchemas.readFile.options[0].shape.value,
  writeFile: responseSchemas.writeFile.options[0].shape.value,
  beginFileRead: responseSchemas.beginFileRead.options[0].shape.value,
  readFileChunk: responseSchemas.readFileChunk.options[0].shape.value,
  beginFileWrite: responseSchemas.beginFileWrite.options[0].shape.value,
  writeFileChunk: responseSchemas.writeFileChunk.options[0].shape.value,
  finishFileWrite: responseSchemas.finishFileWrite.options[0].shape.value,
  cancelFileTransfer: responseSchemas.cancelFileTransfer.options[0].shape.value,
  writeImageFile: responseSchemas.writeImageFile.options[0].shape.value,
  readFileAsset: responseSchemas.readFileAsset.options[0].shape.value,
  resolveDirectory: responseSchemas.resolveDirectory.options[0].shape.value,
  createDirectory: responseSchemas.createDirectory.options[0].shape.value,
  completeDirectory: responseSchemas.completeDirectory.options[0].shape.value,
  beginTerminalUpload: responseSchemas.beginTerminalUpload.options[0].shape.value,
  writeTerminalUpload: responseSchemas.writeTerminalUpload.options[0].shape.value,
  finishTerminalUpload: responseSchemas.finishTerminalUpload.options[0].shape.value,
  cancelTerminalUpload: responseSchemas.cancelTerminalUpload.options[0].shape.value,
};
export const observerResponseSchemas = {
  getSnapshot: reply(observerValueSchemas.getSnapshot),
  getSessionStats: reply(observerValueSchemas.getSessionStats),
  createSession: reply(observerValueSchemas.createSession),
  addTab: reply(observerValueSchemas.addTab),
  activateSession: reply(observerValueSchemas.activateSession),
  renameSession: reply(observerValueSchemas.renameSession),
  renameTab: reply(observerValueSchemas.renameTab),
  reorderTabs: reply(observerValueSchemas.reorderTabs),
  setSessionPinned: reply(observerValueSchemas.setSessionPinned),
  settleSession: reply(observerValueSchemas.settleSession),
  unsettleSession: reply(observerValueSchemas.unsettleSession),
  clearSessionError: reply(observerValueSchemas.clearSessionError),
  setSessionEnv: reply(observerValueSchemas.setSessionEnv),
  closeTab: reply(observerValueSchemas.closeTab),
  attachTerminal: reply(observerValueSchemas.attachTerminal),
  acknowledgeTerminal: reply(observerValueSchemas.acknowledgeTerminal),
  detachTerminal: reply(observerValueSchemas.detachTerminal),
  getTerminalProfiles: reply(observerValueSchemas.getTerminalProfiles),
  getHomeDirectory: reply(observerValueSchemas.getHomeDirectory),
  deleteSession: reply(observerValueSchemas.deleteSession),
  setTerminalEnv: reply(observerValueSchemas.setTerminalEnv),
  setVisibleSessions: reply(observerValueSchemas.setVisibleSessions),
  takeTerminalControl: reply(observerValueSchemas.takeTerminalControl),
  writeTerminal: reply(observerValueSchemas.writeTerminal),
  resizeTerminal: reply(observerValueSchemas.resizeTerminal),
  listFiles: reply(observerValueSchemas.listFiles),
  readFile: reply(observerValueSchemas.readFile),
  writeFile: reply(observerValueSchemas.writeFile),
  beginFileRead: reply(observerValueSchemas.beginFileRead),
  readFileChunk: reply(observerValueSchemas.readFileChunk),
  beginFileWrite: reply(observerValueSchemas.beginFileWrite),
  writeFileChunk: reply(observerValueSchemas.writeFileChunk),
  finishFileWrite: reply(observerValueSchemas.finishFileWrite),
  cancelFileTransfer: reply(observerValueSchemas.cancelFileTransfer),
  writeImageFile: reply(observerValueSchemas.writeImageFile),
  readFileAsset: reply(observerValueSchemas.readFileAsset),
  resolveDirectory: reply(observerValueSchemas.resolveDirectory),
  createDirectory: reply(observerValueSchemas.createDirectory),
  completeDirectory: reply(observerValueSchemas.completeDirectory),
  beginTerminalUpload: reply(observerValueSchemas.beginTerminalUpload),
  writeTerminalUpload: reply(observerValueSchemas.writeTerminalUpload),
  finishTerminalUpload: reply(observerValueSchemas.finishTerminalUpload),
  cancelTerminalUpload: reply(observerValueSchemas.cancelTerminalUpload),
};
export type ObserverPayload<M extends ObserverMethod> = z.input<(typeof observerRequestSchemas)[M]>;
export type ObserverResponse<M extends ObserverMethod> =
  | { version: 2; ok: true; value: z.output<(typeof observerValueSchemas)[M]> }
  | { version: 2; ok: false; error: ObserverError };
const parsers: { [M in ObserverMethod]: { parse(raw: unknown): ObserverResponse<M> } } = observerResponseSchemas;
export function parseObserverResponse<M extends ObserverMethod>(method: M, raw: unknown): ObserverResponse<M> {
  return parsers[method].parse(raw);
}
const commandHeader = z.object({ method: z.enum(observerMethods), payload: z.unknown() }).strict();
export function parseObserverCommand(raw: unknown) {
  const command = commandHeader.parse(raw);
  return { method: command.method, payload: observerRequestSchemas[command.method].parse(command.payload) };
}
export const requestEnvelopeSchema = z.object({ version: z.literal(2), binding: bindingSchema,
  method: z.enum(observerMethods), payload: z.unknown(),
}).strict().superRefine((value, ctx) => {
  if (!observerRequestSchemas[value.method].safeParse(value.payload).success) {
    ctx.addIssue({ code: 'custom', message: 'Invalid observer-v2 payload.' });
  }
});
export function managerCommand<M extends ObserverManagerMethod>(method: M, payload: ObserverPayload<M>): string {
  const nativeOnly = new Set<string>([...deniedMethods, 'acknowledgeTerminal', 'attachTerminal', 'detachTerminal']);
  if (nativeOnly.has(method)) {
    throw new Error('observer-v2 has no input, layout, upload or caller-ACK authority.');
  }
  const command = parseObserverCommand({ method, payload });
  const json = JSON.stringify(command);
  if (new TextEncoder().encode(json).byteLength > 1024 * 1024) throw new Error('Observer request exceeds limit.');
  return json;
}
