import { spawn, type ChildProcess } from 'node:child_process';
import type { KeepAwakeState } from '../shared/contracts';

/** Subset of Electron's powerSaveBlocker, injected so the module stays testable. */
export interface PowerBlocker {
  start(type: 'prevent-app-suspension' | 'prevent-display-sleep'): number;
  stop(id: number): void;
}
type Spawn = (command: string, args: string[]) => ChildProcess;

// Windows helper: refresh the execution state and send a tiny reversible mouse
// move every 30 s. Idle-lock policies look for real input, which the execution
// state alone does not provide. Exits on its own when Shellfox is gone.
const WINDOWS_HELPER = `param([int]$Owner)
$sig = @"
using System;
using System.Runtime.InteropServices;
public static class ShellfoxKeepAwake {
  [DllImport("kernel32.dll")] public static extern uint SetThreadExecutionState(uint f);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, UIntPtr e);
}
"@
try { Add-Type -TypeDefinition $sig -ErrorAction Stop } catch { }
$flags = [uint32]"0x80000043"
try {
  while (Get-Process -Id $Owner -ErrorAction SilentlyContinue) {
    [ShellfoxKeepAwake]::SetThreadExecutionState($flags) | Out-Null
    [ShellfoxKeepAwake]::mouse_event(1, 1, 0, 0, [UIntPtr]::Zero)
    [ShellfoxKeepAwake]::mouse_event(1, -1, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Seconds 30
  }
} finally { [ShellfoxKeepAwake]::SetThreadExecutionState([uint32]"0x80000000") | Out-Null }`;

/** OS helper process that mirrors keep-awake.sh; it must die with Shellfox. */
export function helperCommand(platform: NodeJS.Platform, pid: number): { command: string; args: string[] } | null {
  if (platform === 'darwin') return { command: 'caffeinate', args: ['-dimsu', '-w', String(pid)] };
  // `cat` holds the inhibitor until our stdin pipe closes, including on a crash.
  if (platform === 'linux') return { command: 'systemd-inhibit', args: ['--what=idle:sleep', '--who=Shellfox', '--why=Keep awake', '--mode=block', 'cat'] };
  if (platform === 'win32') return { command: 'powershell.exe', args: ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-WindowStyle', 'Hidden', '-EncodedCommand', Buffer.from(`& { ${WINDOWS_HELPER} } -Owner ${pid}`, 'utf16le').toString('base64')] };
  return null;
}

export class KeepAwake {
  private blocker: number | null = null;
  private helper: ChildProcess | null = null;
  private readonly listeners = new Set<(state: KeepAwakeState) => void>();
  constructor(private readonly power: PowerBlocker, private readonly options: { platform?: NodeJS.Platform; pid?: number; spawn?: Spawn } = {}) {}
  get active(): boolean { return this.blocker !== null; }
  subscribe(listener: (state: KeepAwakeState) => void): () => void {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  }
  set(active: boolean): boolean {
    if (active === this.active) return this.active;
    if (active) {
      // Electron covers display + system sleep on every OS (SetThreadExecutionState /
      // IOPMAssertion / GNOME + freedesktop D-Bus inhibitors).
      this.blocker = this.power.start('prevent-display-sleep');
      this.startHelper();
    } else {
      const id = this.blocker;
      if (id !== null) this.power.stop(id);
      this.blocker = null;
      this.stopHelper();
    }
    const state = Object.freeze({ active: this.active });
    for (const listener of [...this.listeners]) {
      try { listener(state); } catch { /* Observers cannot roll back the switch or suppress other listeners. */ }
    }
    return this.active;
  }
  dispose(): void { this.set(false); this.listeners.clear(); }
  private startHelper(): void {
    const target = helperCommand(this.options.platform ?? process.platform, this.options.pid ?? process.pid);
    if (!target) return;
    try {
      const child = (this.options.spawn ?? ((command, args) => spawn(command, args, { stdio: ['pipe', 'ignore', 'ignore'], windowsHide: true })))(target.command, target.args);
      child.on('error', () => { if (this.helper === child) this.helper = null; }); // Missing tool: Electron's blocker still applies.
      child.on('exit', () => { if (this.helper === child) this.helper = null; });
      child.stdin?.on('error', () => {});
      this.helper = child;
    } catch { this.helper = null; }
  }
  private stopHelper(): void {
    const child = this.helper; this.helper = null;
    if (!child) return;
    try { child.stdin?.end(); } catch { /* already closed */ }
    try { child.kill(); } catch { /* already gone */ }
  }
}
