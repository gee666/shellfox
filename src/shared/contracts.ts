export type Id = string;
export type Timestamp = string;
export type Status = 'waiting' | 'error' | 'running' | 'unknown' | 'settled';
export type TabStatus = Exclude<Status, 'settled'>;
export type TabLifecycle = 'launching' | 'open' | 'closed' | 'launch-uncertain';
export type AdapterId = 'windows-terminal' | 'gnome-terminal' | 'embedded-pty';
export type ShellId = 'pwsh' | 'windows-powershell' | 'bash' | 'login-shell' | 'wsl';
export type ErrorCode = 'VALIDATION' | 'NOT_FOUND' | 'UNSUPPORTED' | 'DEPENDENCY_MISSING' | 'LAUNCH_FAILED' | 'REGISTRATION_TIMEOUT' | 'TARGET_LOST' | 'TARGET_AMBIGUOUS' | 'FOCUS_DENIED' | 'MONITOR_UNAVAILABLE' | 'NATIVE_UNAVAILABLE' | 'AUTH_FAILED' | 'SETTLE_CONFIRM_REQUIRED' | 'RETRY_CONFIRM_REQUIRED' | 'STORAGE_FAILED' | 'INTERNAL';
export interface AppError { code: ErrorCode; message: string; retryable: boolean }
export type Result<T> = { ok: true; value: T } | { ok: false; error: AppError };
export interface Counts { running: number; waiting: number; unknown: number; error: number; closed: number; agents: number }
export interface Capabilities {
  // Capabilities describe the installed backend. Windows automatic appends currently report false.
  createWindow: boolean; addTab: boolean; focusWindow: boolean;
  activateTab: boolean; splitPane: false; attachExisting: false; closeTerminal: boolean;
  commandExitStatus: false; processTracking: boolean; explorerContextMenu: boolean;
  embeddedTerminal?: boolean; terminalLifetime?: 'app-owned' | 'external-legacy'; shellSurvival?: boolean;
}
export interface TabDto {
  id: Id; sessionId: Id; title: string; cwd: string; ordinal: number;
  createdAt: Timestamp; lifecycle: TabLifecycle; status: TabStatus; agents: number;
  monitoringReason: string | null; error: AppError | null;
  generation?: string; terminalKind?: 'embedded' | 'external-legacy'; profileId?: string | null; exitCode?: number | null;
}
export interface SessionWindowDto {
  state: 'alive' | 'closed' | 'unknown' | 'opening' | 'launch-uncertain' | 'unsupported';
  canReopen: boolean; canRegister: boolean; reason: string;
}
export interface RegistrationGuideDto { command: string; expiresAt: Timestamp; titleMarker: string; instructions: string }
export interface EnvVar { name: string; value: string }
export interface CliIntegrationDto { supported: boolean; installed: boolean; command: string; reason: string | null }
export interface SessionDto {
  id: Id; title: string; cwd: string; adapterId: AdapterId; shellId: ShellId;
  createdAt: Timestamp; updatedAt: Timestamp; settledAt: Timestamp | null;
  status: Status; activityStatus: TabStatus; counts: Counts; tabs: TabDto[]; env: EnvVar[];
  /** Pinned sessions stay at the top of the live list, in pin order. Archived sessions remember it without effect. */
  pinnedAt: Timestamp | null;
  canFocus: boolean; canAddTab: boolean; controlReason: string | null; error: AppError | null;
  window?: SessionWindowDto;
  terminalLifetime?: 'app-owned' | 'external-legacy'; shellSurvival?: boolean;
}
export interface ProcessRule {
  id: Id; label: string; enabled: boolean; executableBasenames: string[];
  executablePaths: string[]; scriptPathSuffixes: string[];
}
export interface SettingsDto {
  version: 1; pythonPath?: string | null; accentColor: string; backgroundColor: string; adapterId: AdapterId; shellId: ShellId;
  shellExecutable: string | null; processRules: ProcessRule[]; historyPageSize: number;
  terminalProfileId?: string | null;
}
export interface ShellOption { id: ShellId; executable: string; available: boolean; reason: string | null }
export interface NativeProbe {
  platform: string; arch: string; adapterId: AdapterId; available: boolean;
  python?: { detected: string | null; usable: boolean; reason: string | null };
  terminalVersion: string | null; capabilities: Capabilities; shells: ShellOption[]; reasons: string[];
}
export interface ExplorerIntegrationDto {
  supported: boolean; installed: boolean; folderItemInstalled: boolean;
  backgroundInstalled: boolean; reason: string | null;
}
export interface HistoryQuery { search: string; status: 'all' | Status; page: number; pageSize: number }
export interface HistoryPage { items: SessionDto[]; total: number; page: number; pageSize: number }
export interface ManagerSnapshot {
  revision: number; sessions: SessionDto[]; settings: SettingsDto; probe: NativeProbe; explorer: ExplorerIntegrationDto; cli: CliIntegrationDto;
}
export interface ChangedEvent { revision: number; reason: 'sessions' | 'settings' | 'native' | 'history'; selectSessionId?: Id }
export interface TerminalProfileDto {
  id: string; label: string; environment: 'local' | 'wsl'; executable: string;
  args: string[]; distro: string | null; available: boolean;
  unavailableReason?: string | null; canTerminateDescendants?: boolean;
}
export interface TerminalProfilesDto {
  profiles: TerminalProfileDto[]; defaultProfileId: string | null; lifetime: 'app-owned'; shellSurvival: false;
}
export interface TerminalDataEvent { type: 'data'; tabId: Id; generation: Id; sequence: number; data: string }
export type TerminalEvent = TerminalDataEvent
  | { type: 'activity'; tabId: Id; busy: boolean }
  | { type: 'exit'; tabId: Id; generation: Id; exitCode: number | null; signal: number | null; lastSequence: number }
  | { type: 'error'; tabId: Id; generation: Id; error: AppError };
export interface TerminalAttachmentDto {
  tabId: Id; sessionId: Id; generation: Id; firstSequence: number; lastSequence: number;
  chunks: TerminalDataEvent[]; truncated: boolean; state: 'open' | 'closed'; exitCode: number | null;
  cols: number; rows: number; lifetime: 'app-owned';
}
export interface TerminalApi {
  getTerminalProfiles(): Promise<Result<TerminalProfilesDto>>;
  attachTerminal(input: { tabId: Id; afterSequence?: number; generation?: Id }): Promise<Result<TerminalAttachmentDto>>;
  writeTerminal(input: { tabId: Id; generation: Id; data: string }): Promise<Result<{ written: true }>>;
  resizeTerminal(input: { tabId: Id; generation: Id; cols: number; rows: number }): Promise<Result<{ resized: true }>>;
  closeTab(input: { tabId: Id; generation: Id }): Promise<Result<SessionDto>>;
  acknowledgeTerminal(input: { tabId: Id; generation: Id; sequence: number }): Promise<Result<{ acknowledged: true }>>;
  detachTerminal(input: { tabId: Id; generation: Id }): Promise<Result<{ detached: true }>>;
  subscribeTerminal(listener: (event: TerminalEvent) => void): () => void;
}
// Optional only for legacy injected clients. The production preload exposes every terminal method.
export interface UpdateStatusDto { current: string; latest: string | null; available: boolean; command: 'shellfox update'; url: string }
export interface ManagerApi extends Partial<TerminalApi> {
  getSnapshot(): Promise<Result<ManagerSnapshot>>;
  activateSession(input: { sessionId: Id }): Promise<Result<SessionDto>>;
  refreshSessionMembership(input: { sessionId: Id }): Promise<Result<SessionDto>>;
  prepareSessionRegistration(input: { sessionId: Id; shellId: ShellId }): Promise<Result<RegistrationGuideDto>>;
  createSession(input: { title?: string; cwd: string; requestId: Id }): Promise<Result<SessionDto>>;
  addTab(input: { sessionId: Id; title?: string; profileId?: string; cwd?: string }): Promise<Result<SessionDto>>;
  focusSession(input: { sessionId: Id }): Promise<Result<{ focused: true }>>;
  renameSession(input: { sessionId: Id; title: string }): Promise<Result<SessionDto>>;
  setSessionPinned(input: { sessionId: Id; pinned: boolean }): Promise<Result<SessionDto>>;
  settleSession(input: { sessionId: Id; confirmActive: boolean }): Promise<Result<SessionDto>>;
  unsettleSession(input: { sessionId: Id }): Promise<Result<SessionDto>>;
  clearSessionError(input: { sessionId: Id }): Promise<Result<SessionDto>>;
  retryTab(input: { tabId: Id; confirmPossibleDuplicate: boolean }): Promise<Result<SessionDto>>;
  getHistory(query: HistoryQuery): Promise<Result<HistoryPage>>;
  saveSettings(input: SettingsDto): Promise<Result<SettingsDto>>;
  setExplorerIntegration(input: { installed: boolean }): Promise<Result<ExplorerIntegrationDto>>;
  copyText(input: { text: string }): Promise<Result<{ copied: true }>>;
  openSessionFolder(input: { sessionId: Id }): Promise<Result<{ opened: true }>>;
  setSessionEnv(input: { sessionId: Id; env: EnvVar[] }): Promise<Result<SessionDto>>;
  setCliIntegration(input: { installed: boolean }): Promise<Result<CliIntegrationDto>>;
  chooseDirectory(): Promise<Result<{ cwd: string } | null>>;
  getUpdateStatus?(): Promise<Result<UpdateStatusDto>>;
  subscribe(listener: (event: ChangedEvent) => void): () => void;
}
export const success = <T>(value: T): Result<T> => ({ ok: true, value });
export const failure = (code: ErrorCode, message: string, retryable = false): Result<never> =>
  ({ ok: false, error: { code, message: message.slice(0, 1000), retryable } });
