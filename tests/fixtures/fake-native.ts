import type { NativeBackend, NativeEvent, LaunchRequest, WindowTarget, WatchTab, TabObservation } from '../../src/shared/native-port';
import type { NativeProbe, ProcessRule, ExplorerIntegrationDto } from '../../src/shared/contracts';
import { success, failure } from '../../src/shared/contracts';

// Only bundled into tmp/build-test. Never a fallback for the real backend.
export class FakeNativeBackend implements NativeBackend {
  readonly launches: LaunchRequest[] = [];
  readonly focuses: WindowTarget[] = [];
  readonly listeners = new Set<(event: NativeEvent) => void>();
  watch: WatchTab[] = [];
  mode: 'register-before-receipt' | 'register-after-receipt' | 'timeout' | 'failure' | 'pending' = 'register-before-receipt';
  focusDenied = false;
  targetLost = false;
  private serial = 0;
  private timers = new Set<ReturnType<typeof setTimeout>>();
  private explorer: ExplorerIntegrationDto = { supported: false, installed: false, folderItemInstalled: false, backgroundInstalled: false, reason: 'Fake tests cannot register Explorer actions' };
  async initialize() {
    const delay = Number(process.env.SHELLFOX_FAKE_INIT_DELAY_MS ?? 0);
    if (delay > 0 && delay <= 5000) await new Promise(resolve => setTimeout(resolve, delay));
    const probe: NativeProbe = {
      platform: 'win32', arch: 'x64', adapterId: 'windows-terminal', available: true, terminalVersion: 'fixture',
      capabilities: { createWindow: true, addTab: true, focusWindow: true, processTracking: true, explorerContextMenu: false, activateTab: false, splitPane: false, attachExisting: false, closeTerminal: false, commandExitStatus: false },
      shells: [{ id: 'pwsh', executable: 'C:\\Program Files\\PowerShell\\7\\pwsh.exe', available: true, reason: null }, { id: 'windows-powershell', executable: 'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe', available: true, reason: null }], reasons: [],
    };
    return success(probe);
  }
  emit(event: NativeEvent) { for (const listener of this.listeners) listener(event); }
  observe(agents = 0, root: TabObservation['root'] = 'alive', health: TabObservation['health'] = 'healthy') {
    this.emit({ type: 'observations', items: this.watch.map(tab => ({ sessionId: tab.sessionId, tabId: tab.tabId, observedAt: new Date().toISOString(), root, health, agents, reason: health === 'unknown' || root === 'unavailable' ? 'Fixture access unavailable' : null })) });
  }
  async launch(input: LaunchRequest) {
    this.launches.push(structuredClone(input));
    if (this.mode === 'failure') return failure('LAUNCH_FAILED', 'Fixture pre-spawn failure', true);
    const n = ++this.serial;
    const target: WindowTarget = input.existingTarget ?? { kind: 'windows-terminal', windowName: input.windowName, hwnd: String(10000 + n), owner: { pid: 2000 + n, startTime: String(134000000000000000n + BigInt(n)) }, sessionId: input.sessionId, markerPrefix: `SHELLFOX:${input.sessionId}:`, verification: 'native-title' };
    const register = () => this.emit({ type: 'registered', registration: { sessionId: input.sessionId, tabId: input.tabId, operationId: input.operationId, shell: { pid: 3000 + n, startTime: String(134000000000000000n + BigInt(n)) }, shellExecutable: input.shellExecutable, cwd: input.cwd, registeredAt: new Date().toISOString() }, target });
    if (this.mode === 'register-before-receipt') register();
    if (this.mode === 'register-after-receipt') {
      const timer = setTimeout(() => { this.timers.delete(timer); register(); }, 50);
      this.timers.add(timer);
    }
    if (this.mode === 'timeout') this.emit({ type: 'operation-error', sessionId: input.sessionId, tabId: input.tabId, operationId: input.operationId, error: { code: 'REGISTRATION_TIMEOUT', message: 'Fixture registration deadline', retryable: true } });
    // Null receipt deliberately exercises registration arriving before the receipt.
    return success({ operationId: input.operationId, dispatch: 'started' as const, target: null });
  }
  async focus(input: WindowTarget) { this.focuses.push(structuredClone(input)); return this.focusDenied ? failure('FOCUS_DENIED', 'Fixture foreground denial. Select the terminal manually.', true) : success({ focused: true as const }); }
  async verifyTarget(input: WindowTarget) { return this.targetLost ? failure('TARGET_LOST', 'Fixture stale HWND', true) : success(input); }
  async setWatch(input: { tabs: WatchTab[]; rules: ProcessRule[] }) { this.watch = structuredClone(input.tabs); return success({ configured: true as const }); }
  async getExplorerIntegration() { return success(this.explorer); }
  async setExplorerIntegration() { return failure('UNSUPPORTED', 'Fake tests cannot change Explorer registration'); }
  subscribe(listener: (event: NativeEvent) => void) { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; }
  async dispose() { for (const timer of this.timers) clearTimeout(timer); this.timers.clear(); this.listeners.clear(); }
}
export function createFakeNativeBackend(): NativeBackend { return new FakeNativeBackend(); }
