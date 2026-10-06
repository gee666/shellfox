import { join } from 'node:path';
import { spawn } from 'node:child_process';
import { z } from 'zod';
import { failure, success, type Capabilities, type ExplorerIntegrationDto, type NativeProbe, type ProcessRule, type Result } from '../../shared/contracts';
import type { LaunchReceipt, LaunchRequest, NativeBackend, NativeEvent, NativeInit, WatchTab, WindowTarget } from '../../shared/native-port';
import { explorerSchema, idSchema, launchReceiptSchema, pathSchema, probeSchema, processRuleSchema, registrationSchema, windowTargetSchema } from '../../shared/schemas';
import { BrokerTransport } from './transport';
import { boundWindowSchema, memberRegistrationSchema, membershipSchema, instructionsSchema, type BoundWindow, type MemberRegistration, type SessionWindowBackend, type SessionWindowEvent } from './session-windows';

const disabled: Capabilities = { createWindow: false, addTab: false, focusWindow: false, activateTab: false, splitPane: false, attachExisting: false, closeTerminal: false, commandExitStatus: false, processTracking: false, explorerContextMenu: false };
const initSchema = z.object({ userDataDir: pathSchema, helperDir: pathSchema, shellScriptDir: pathSchema, packagedExecutable: pathSchema.nullable() }).strict();
const launchSchema = z.object({ sessionId: idSchema, tabId: idSchema, operationId: idSchema, cwd: pathSchema, shellId: z.enum(['pwsh', 'windows-powershell', 'bash']), shellExecutable: pathSchema, windowName: z.string().max(100), titleMarker: z.string().max(200), existingTarget: windowTargetSchema.nullable() }).strict();
const watchSchema = z.object({ tabs: z.array(z.object({ sessionId: idSchema, tabId: idSchema, registration: registrationSchema }).strict()).max(1000), rules: z.array(processRuleSchema).max(100) }).strict();
const explorerRequestSchema = z.object({ installed: z.boolean(), executablePath: pathSchema }).strict();
const unsupported = () => failure('UNSUPPORTED', 'Native sessions require Windows x64 with Windows Terminal and supported PowerShell.');

export class WindowsBackend implements SessionWindowBackend {
  private readonly listeners = new Set<(event: NativeEvent) => void>();
  private readonly windowListeners = new Set<(event: SessionWindowEvent) => void>();
  private readonly transport = new BrokerTransport(event => this.emit(event), event => {
    for (const listener of this.windowListeners) { try { listener(event); } catch { /* One subscriber must not break framing. */ } }
  });
  constructor(private readonly helperName = 'Shellfox.Native.exe') {}
  private helperExecutable: string | null = null;
  private foregroundUntil = 0;
  // Called only by trusted main IPC when its BrowserWindow is currently focused.
  authorizeForegroundFocus(): void { if (process.platform === 'win32') this.foregroundUntil = Date.now() + 4000; }
  private async delegateForeground(): Promise<void> {
    const pid = this.transport.processId;
    if (process.platform !== 'win32' || Date.now() > this.foregroundUntil || !pid || !this.helperExecutable) return;
    this.foregroundUntil = 0;
    await new Promise<void>(resolve => {
      const child = spawn(this.helperExecutable!, ['grant-foreground', '--broker-pid', String(pid)], { shell: false, windowsHide: true, stdio: 'ignore' });
      const timer = setTimeout(() => { child.kill(); resolve(); }, 2000);
      child.once('error', () => { clearTimeout(timer); resolve(); });
      child.once('exit', () => { clearTimeout(timer); resolve(); });
    });
    // Grant denial is not success. The broker still verifies the target and actual foreground.
  }
  private initialized = false;
  private disposed = false;
  private emit(event: NativeEvent): void {
    for (const listener of this.listeners) { try { listener(event); } catch { /* A subscriber cannot break native framing. */ } }
  }
  subscribe(listener: (event: NativeEvent) => void): () => void { this.listeners.add(listener); return () => this.listeners.delete(listener); }

  async initialize(input: NativeInit): Promise<Result<NativeProbe>> {
    if (this.initialized || this.disposed) return failure('NATIVE_UNAVAILABLE', 'Native backend is already initialized or disposed.');
    const parsed = initSchema.safeParse(input);
    if (!parsed.success) return failure('VALIDATION', 'Invalid native resource paths.');
    this.initialized = true;
    this.helperExecutable = join(input.helperDir, this.helperName);
    try { this.transport.start(this.helperExecutable); }
    catch { return failure('NATIVE_UNAVAILABLE', 'Native helper could not be started.'); }
    return this.transport.request('initialize', parsed.data, probeSchema);
  }
  async launch(input: LaunchRequest): Promise<Result<LaunchReceipt>> {
    const parsed = launchSchema.safeParse(input);
    if (!parsed.success || input.windowName !== `shellfox-${input.sessionId}` || input.titleMarker !== `SHELLFOX:${input.sessionId}:${input.tabId}` || input.existingTarget && input.existingTarget.sessionId !== input.sessionId) return failure('VALIDATION', 'Invalid generated launch identity.');
    if (input.existingTarget) return failure('UNSUPPORTED', 'Adding tabs is disabled because Windows Terminal target lookup can create an unexpected window. No append was dispatched.');
    return this.transport.request('launch', parsed.data, launchReceiptSchema);
  }
  async focus(input: WindowTarget): Promise<Result<{ focused: true }>> {
    const parsed = windowTargetSchema.safeParse(input);
    if (!parsed.success) return failure('VALIDATION', 'Invalid window target.');
    await this.delegateForeground();
    return this.transport.request('focus', parsed.data, z.object({ focused: z.literal(true) }).strict());
  }
  async verifyTarget(input: WindowTarget): Promise<Result<WindowTarget>> {
    const parsed = windowTargetSchema.safeParse(input);
    if (!parsed.success) return failure('VALIDATION', 'Invalid window target.');
    return this.transport.request('verifyTarget', parsed.data, windowTargetSchema);
  }
  async setWatch(input: { tabs: WatchTab[]; rules: ProcessRule[] }): Promise<Result<{ configured: true }>> {
    const parsed = watchSchema.safeParse(input);
    if (!parsed.success) return failure('VALIDATION', 'Invalid process-watch configuration.');
    return this.transport.request('setWatch', parsed.data, z.object({ configured: z.literal(true) }).strict());
  }
  subscribeSessionWindows(listener: (event: SessionWindowEvent) => void): () => void { this.windowListeners.add(listener); return () => this.windowListeners.delete(listener); }
  async reopenWindow(input: { request: LaunchRequest; previous: BoundWindow }): Promise<Result<LaunchReceipt>> {
    const parsed = z.object({ request: launchSchema, previous: boundWindowSchema }).strict().safeParse(input);
    if (!parsed.success || input.request.existingTarget || input.request.sessionId !== input.previous.target.sessionId) return failure('VALIDATION', 'Invalid same-session replacement intent.');
    return this.transport.request('reopenWindow', parsed.data, launchReceiptSchema);
  }
  async prepareRegistration(input: { request: LaunchRequest; binding: BoundWindow }) {
    const parsed = z.object({ request: launchSchema, binding: boundWindowSchema }).strict().safeParse(input);
    if (!parsed.success || input.request.existingTarget || input.request.sessionId !== input.binding.target.sessionId) return failure('VALIDATION', 'Invalid explicit registration intent.');
    return this.transport.request('prepareRegistration', parsed.data, instructionsSchema);
  }
  async focusSessionWindow(input: BoundWindow): Promise<Result<{ focused: true }>> {
    const parsed = boundWindowSchema.safeParse(input); if (!parsed.success) return failure('VALIDATION', 'Invalid window generation.');
    await this.delegateForeground();
    return this.transport.request('focusSessionWindow', parsed.data, z.object({ focused: z.literal(true) }).strict());
  }
  async getWindowMembership(input: BoundWindow) {
    const parsed = boundWindowSchema.safeParse(input); if (!parsed.success) return failure('VALIDATION', 'Invalid window generation.');
    return this.transport.request('getWindowMembership', parsed.data, membershipSchema);
  }
  async restoreSessionWindows(input: { bindings: BoundWindow[]; members: MemberRegistration[] }): Promise<Result<{ restored: true }>> {
    const parsed = z.object({ bindings: z.array(boundWindowSchema).max(1000), members: z.array(memberRegistrationSchema).max(1000) }).strict().safeParse(input);
    if (!parsed.success) return failure('VALIDATION', 'Invalid restored membership.');
    return this.transport.request('restoreSessionWindows', parsed.data, z.object({ restored: z.literal(true) }).strict());
  }
  async getExplorerIntegration(): Promise<Result<ExplorerIntegrationDto>> { return this.transport.request('getExplorerIntegration', {}, explorerSchema); }
  async setExplorerIntegration(input: { installed: boolean; executablePath: string }): Promise<Result<ExplorerIntegrationDto>> {
    const parsed = explorerRequestSchema.safeParse(input);
    if (!parsed.success) return failure('VALIDATION', 'Invalid Explorer installation request.');
    return this.transport.request('setExplorerIntegration', parsed.data, explorerSchema);
  }
  async dispose(): Promise<void> { this.disposed = true; await this.transport.dispose(); this.listeners.clear(); this.windowListeners.clear(); }
}

export class UnsupportedBackend implements NativeBackend {
  async initialize(_input: NativeInit): Promise<Result<NativeProbe>> {
    return success({ platform: process.platform, arch: process.arch, adapterId: 'windows-terminal', available: false, terminalVersion: null, capabilities: { ...disabled }, shells: [], reasons: ['Native sessions are supported only on Windows x64. No terminal was launched.'] });
  }
  async launch(_input: LaunchRequest): Promise<Result<LaunchReceipt>> { return unsupported(); }
  async focus(_input: WindowTarget): Promise<Result<{ focused: true }>> { return unsupported(); }
  async verifyTarget(_input: WindowTarget): Promise<Result<WindowTarget>> { return unsupported(); }
  async setWatch(_input: { tabs: WatchTab[]; rules: ProcessRule[] }): Promise<Result<{ configured: true }>> { return unsupported(); }
  async getExplorerIntegration(): Promise<Result<ExplorerIntegrationDto>> { return success({ supported: false, installed: false, folderItemInstalled: false, backgroundInstalled: false, reason: 'Explorer integration requires Windows x64 and a packaged install.' }); }
  async setExplorerIntegration(_input: { installed: boolean; executablePath: string }): Promise<Result<ExplorerIntegrationDto>> { return unsupported(); }
  subscribe(_listener: (event: NativeEvent) => void): () => void { return () => {}; }
  async dispose(): Promise<void> {}
}

export class LinuxBackend extends WindowsBackend {
  readonly supportsSessionWindows = false;
  constructor() { super('Shellfox.Linux'); }
}
export function createNativeBackend(): NativeBackend {
  if (process.arch === 'x64' && process.platform === 'win32') return new WindowsBackend();
  if (process.arch === 'x64' && process.platform === 'linux') return new LinuxBackend();
  return new UnsupportedBackend();
}
