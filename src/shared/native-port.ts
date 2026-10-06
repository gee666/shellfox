import type { Id, Timestamp, ShellId, AppError, Result, NativeProbe, ExplorerIntegrationDto, ProcessRule } from './contracts';
// Opaque decimal birth identity: Windows exact FILETIME; Linux boot UUID + kernel start ticks.
// Never convert to Number/Date or compare identities across platforms.
export interface ProcessIdentity { pid: number; startTime: string }
export interface ShellRegistration {
  sessionId: Id; tabId: Id; operationId: Id; shell: ProcessIdentity;
  shellExecutable: string; cwd: string; registeredAt: Timestamp;
}
export interface WindowTarget {
  // windowName is an intent tag, not proof of Windows Terminal's internal routing name.
  kind: 'windows-terminal'; windowName: string; hwnd: string; owner: ProcessIdentity;
  sessionId: Id; markerPrefix: string; verification: 'native-title';
}
export interface LaunchRequest {
  sessionId: Id; tabId: Id; operationId: Id; cwd: string; shellId: ShellId;
  shellExecutable: string; windowName: string; titleMarker: string; existingTarget: WindowTarget | null;
}
export interface LaunchReceipt { operationId: Id; dispatch: 'started'; target: WindowTarget | null }
export interface WatchTab { sessionId: Id; tabId: Id; registration: ShellRegistration }
export interface TabObservation {
  sessionId: Id; tabId: Id; observedAt: Timestamp; root: 'alive' | 'exited' | 'unavailable';
  health: 'healthy' | 'unknown'; agents: number; reason: string | null;
}
export type NativeEvent =
  | { type: 'registered'; registration: ShellRegistration; target: WindowTarget | null }
  | { type: 'observations'; items: TabObservation[] }
  | { type: 'target-lost'; sessionId: Id; reason: string }
  | { type: 'operation-error'; sessionId: Id; tabId: Id; operationId: Id; error: AppError }
  | { type: 'unavailable'; error: AppError };
export interface NativeInit {
  userDataDir: string; helperDir: string; shellScriptDir: string; packagedExecutable: string | null;
}
export interface NativeBackend {
  initialize(input: NativeInit): Promise<Result<NativeProbe>>;
  launch(input: LaunchRequest): Promise<Result<LaunchReceipt>>;
  focus(input: WindowTarget): Promise<Result<{ focused: true }>>;
  verifyTarget(input: WindowTarget): Promise<Result<WindowTarget>>;
  setWatch(input: { tabs: WatchTab[]; rules: ProcessRule[] }): Promise<Result<{ configured: true }>>;
  getExplorerIntegration(): Promise<Result<ExplorerIntegrationDto>>;
  setExplorerIntegration(input: { installed: boolean; executablePath: string }): Promise<Result<ExplorerIntegrationDto>>;
  subscribe(listener: (event: NativeEvent) => void): () => void;
  dispose(): Promise<void>;
}
