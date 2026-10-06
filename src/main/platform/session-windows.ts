import { z } from 'zod';
import type { Result } from '../../shared/contracts';
import type { NativeBackend, LaunchRequest, LaunchReceipt, ShellRegistration, WindowTarget } from '../../shared/native-port';
import { idSchema, registrationSchema, windowTargetSchema, pathSchema, timestampSchema } from '../../shared/schemas';

export interface BoundWindow { target: WindowTarget; generation: string }
export interface MemberRegistration { registration: ShellRegistration; wtSession: string | null; binding: BoundWindow }
export interface WindowMembership { binding: BoundWindow; members: MemberRegistration[]; windowState: 'alive' | 'closed' | 'unavailable'; discovery: 'explicit-registration'; reason: string }
export interface RegistrationInstructions { ticketPath: string; scriptPath: string; expiresAt: string; titleMarker: string }
export type SessionWindowEvent =
  | { type: 'bound'; binding: BoundWindow }
  | { type: 'lost'; binding: BoundWindow; reason: string }
  | { type: 'adopted'; member: MemberRegistration; previous: MemberRegistration | null; source: 'launch' | 'manual' }
  | { type: 'closed'; member: MemberRegistration };
export interface SessionWindowBackend extends NativeBackend {
  reopenWindow(input: { request: LaunchRequest; previous: BoundWindow }): Promise<Result<LaunchReceipt>>;
  prepareRegistration(input: { request: LaunchRequest; binding: BoundWindow }): Promise<Result<RegistrationInstructions>>;
  focusSessionWindow(input: BoundWindow): Promise<Result<{ focused: true }>>;
  getWindowMembership(input: BoundWindow): Promise<Result<WindowMembership>>;
  restoreSessionWindows(input: { bindings: BoundWindow[]; members: MemberRegistration[] }): Promise<Result<{ restored: true }>>;
  subscribeSessionWindows(listener: (event: SessionWindowEvent) => void): () => void;
}
export const boundWindowSchema = z.object({ target: windowTargetSchema, generation: idSchema }).strict();
export const memberRegistrationSchema = z.object({ registration: registrationSchema, wtSession: idSchema.nullable(), binding: boundWindowSchema }).strict();
export const membershipSchema = z.object({ binding: boundWindowSchema, members: z.array(memberRegistrationSchema).max(1000), windowState: z.enum(['alive', 'closed', 'unavailable']), discovery: z.literal('explicit-registration'), reason: z.string().max(1000) }).strict();
export const instructionsSchema = z.object({ ticketPath: pathSchema, scriptPath: pathSchema, expiresAt: timestampSchema, titleMarker: z.string().max(200) }).strict();
export const sessionWindowEventSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('bound'), binding: boundWindowSchema }).strict(),
  z.object({ type: z.literal('lost'), binding: boundWindowSchema, reason: z.string().max(1000) }).strict(),
  z.object({ type: z.literal('adopted'), member: memberRegistrationSchema, previous: memberRegistrationSchema.nullable(), source: z.enum(['launch', 'manual']) }).strict(),
  z.object({ type: z.literal('closed'), member: memberRegistrationSchema }).strict(),
]);
export function hasSessionWindowBackend(backend: NativeBackend): backend is SessionWindowBackend {
  if ('supportsSessionWindows' in backend && backend.supportsSessionWindows === false) return false;
  return typeof (backend as Partial<SessionWindowBackend>).reopenWindow === 'function';
}
