import type { AdapterId, AppError, EnvVar, SettingsDto, ShellId, TabLifecycle } from '../shared/contracts';
import type { BoundWindow, MemberRegistration } from './platform/session-windows';
import type { ShellRegistration, WindowTarget } from '../shared/native-port';
export interface SessionRecord {
  id: string; title: string; cwd: string; adapterId: AdapterId; shellId: ShellId;
  shellExecutable: string; createdAt: string; updatedAt: string; settledAt: string | null;
  error: AppError | null; target: WindowTarget | null;
  env?: EnvVar[];
  /** Set while pinned to the top of the live list. Kept when archived (no effect there). */
  pinnedAt?: string | null;
  binding?: BoundWindow | null; windowState?: 'alive' | 'closed' | 'unknown' | 'opening' | 'launch-uncertain';
}
export interface TabRecord {
  id: string; sessionId: string; title: string; cwd: string; ordinal: number; createdAt: string;
  lifecycle: TabLifecycle; operationId: string; registration: ShellRegistration | null; error: AppError | null;
  member?: MemberRegistration | null;
  terminal?: { kind: 'embedded'; profileId: string; exitCode: number | null } | null;
}
export interface OperationRecord {
  id: string; sessionId: string; tabId: string | null; requestId: string | null;
  kind: 'create' | 'add' | 'retry' | 'reopen' | 'adopt'; state: 'intent' | 'dispatched' | 'registered' | 'failed' | 'uncertain';
  createdAt: string; updatedAt: string; error: AppError | null;
}
export interface RepositoryPort {
  sessions(): SessionRecord[]; tabs(sessionId?: string): TabRecord[]; session(id: string): SessionRecord | undefined;
  tab(id: string): TabRecord | undefined; saveSession(s: SessionRecord): void; saveTab(t: TabRecord): void;
  /** Deletes an archived session with its tabs, operations and metadata. False when it is missing or not archived. */
  deleteSession(id: string): boolean;
  saveOperation(o: OperationRecord): void; operation(id: string): OperationRecord | undefined;
  operationsForTab(tabId: string): OperationRecord[];
  sessionByRequest(requestId: string): SessionRecord | undefined;
  transaction<T>(fn: () => T): T;
  settings(): SettingsDto; saveSettings(settings: SettingsDto): void;
  settled(search: string): SessionRecord[];
  explorerPreference(): boolean; saveExplorerPreference(installed: boolean): void;
  cliPreference?(): boolean | null; saveCliPreference?(installed: boolean): void; close(): void;
}
