import { spawn, type ChildProcess } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { existsSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import type { KeepAwakeState } from '../shared/contracts';

/** Subset of Electron's powerSaveBlocker, injected so the module stays testable. */
export interface PowerBlocker {
  start(type: 'prevent-app-suspension' | 'prevent-display-sleep'): number;
  stop(id: number): void;
}
type Spawn = (command: string, args: string[]) => ChildProcess;
export interface HelperSpec { command: string; args: string[]; stopFile?: string }

// Windows helper. Every 30 s it refreshes the execution state and sends a tiny
// reversible mouse move (idle-lock policies look for real input). It also sets
// the lid-close action to "do nothing" and restores the saved original when
// told to stop, when Shellfox is gone, or on the next run after a hard kill.
const WINDOWS_HELPER = `param([int]$Owner, [string]$State, [string]$Stop, [switch]$RestoreOnly)
function Get-Lid { $q = (powercfg /q SCHEME_CURRENT SUB_BUTTONS LIDACTION 2>$null) | Out-String; $m = [regex]::Matches($q, '0x[0-9a-fA-F]{8}'); if ($m.Count -ge 2) { ,@([Convert]::ToInt32($m[$m.Count - 2].Value, 16), [Convert]::ToInt32($m[$m.Count - 1].Value, 16)) } }
function Set-Lid([int]$Ac, [int]$Dc) { powercfg /setacvalueindex SCHEME_CURRENT SUB_BUTTONS LIDACTION $Ac 2>$null | Out-Null; powercfg /setdcvalueindex SCHEME_CURRENT SUB_BUTTONS LIDACTION $Dc 2>$null | Out-Null; powercfg /setactive SCHEME_CURRENT 2>$null | Out-Null }
function Read-Saved { try { if ($State -and (Test-Path -LiteralPath $State)) { $v = @((Get-Content -LiteralPath $State -Raw).Trim() -split ',' | ForEach-Object { [int]$_ }); if ($v.Count -eq 2) { return ,$v } } } catch { }; $null }
function Restore-Lid($Lid) { if ($Lid) { try { Set-Lid $Lid[0] $Lid[1] } catch { } }; if ($State) { Remove-Item -LiteralPath $State -Force -ErrorAction SilentlyContinue } }
if ($RestoreOnly) { Restore-Lid (Read-Saved); exit 0 }
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
$lid = Read-Saved
try {
  if (-not $lid) { $lid = Get-Lid; if ($lid -and $State) { Set-Content -LiteralPath $State -Value ($lid -join ',') } }
  if ($lid) { Set-Lid 0 0 }
} catch { }
try {
  $tick = 0
  while ((Get-Process -Id $Owner -ErrorAction SilentlyContinue) -and -not ($Stop -and (Test-Path -LiteralPath $Stop))) {
    if ($tick % 30 -eq 0) {
      [ShellfoxKeepAwake]::SetThreadExecutionState($flags) | Out-Null
      [ShellfoxKeepAwake]::mouse_event(1, 1, 0, 0, [UIntPtr]::Zero)
      [ShellfoxKeepAwake]::mouse_event(1, -1, 0, 0, [UIntPtr]::Zero)
    }
    $tick++
    Start-Sleep -Seconds 1
  }
} finally {
  [ShellfoxKeepAwake]::SetThreadExecutionState([uint32]"0x80000000") | Out-Null
  Restore-Lid $lid
  if ($Stop) { Remove-Item -LiteralPath $Stop -Force -ErrorAction SilentlyContinue }
}`;

function powershell(script: string): HelperSpec {
  return { command: 'powershell.exe', args: ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-WindowStyle', 'Hidden', '-EncodedCommand', Buffer.from(script, 'utf16le').toString('base64')] };
}
const psString = (value: string) => `'${value.replace(/'/g, "''")}'`;

/** OS helper processes that complement Electron's blocker; each must die with Shellfox. */
export function helperCommands(platform: NodeJS.Platform, pid: number, windows: { state?: string; stop?: string } = {}): HelperSpec[] {
  // Idle, display, disk and system sleep; -s is honoured on AC only. Lid-close sleep needs root (pmset) and is not overridden.
  if (platform === 'darwin') return [{ command: 'caffeinate', args: ['-dimsu', '-w', String(pid)] }];
  if (platform === 'linux') {
    // `cat` holds each inhibitor until our stdin pipe closes, including on a crash.
    // logind ignores sleep inhibitors for the lid switch (LidSwitchIgnoreInhibited=yes),
    // so closing the lid would still suspend and lock; handle-lid-switch is separate
    // so a policy refusing it cannot take the idle/sleep lock down with it.
    const inhibit = (what: string) => ({ command: 'systemd-inhibit', args: [`--what=${what}`, '--who=Shellfox', '--why=Keep awake', '--mode=block', 'cat'] });
    return [inhibit('idle:sleep'), inhibit('handle-lid-switch')];
  }
  if (platform === 'win32') {
    const args = [`-Owner ${pid}`, windows.state ? `-State ${psString(windows.state)}` : '', windows.stop ? `-Stop ${psString(windows.stop)}` : ''].filter(Boolean).join(' ');
    return [{ ...powershell(`& { ${WINDOWS_HELPER} } ${args}`), ...(windows.stop ? { stopFile: windows.stop } : {}) }];
  }
  return [];
}

/** Restores a lid-close setting left behind by a hard-killed Windows helper. */
export function windowsRestoreCommand(state: string): HelperSpec {
  return powershell(`& { ${WINDOWS_HELPER} } -State ${psString(state)} -RestoreOnly`);
}

const RETRY_MS = 30_000, STOP_GRACE_MS = 10_000;
interface Helper { spec: HelperSpec; child: ChildProcess | null; retry: ReturnType<typeof setTimeout> | null; missing: boolean }

export class KeepAwake {
  private blocker: number | null = null;
  private helpers: Helper[] = [];
  private readonly stopping = new Set<ChildProcess>();
  private afterStop: (() => void)[] = [];
  private readonly listeners = new Set<(state: KeepAwakeState) => void>();
  private readonly platform: NodeJS.Platform;
  constructor(private readonly power: PowerBlocker, private readonly options: { platform?: NodeJS.Platform; pid?: number; spawn?: Spawn; stateDir?: string } = {}) {
    this.platform = options.platform ?? process.platform;
    const state = this.windowsState();
    if (state && existsSync(state)) { // Previous helper was hard-killed.
      const child = this.run(windowsRestoreCommand(state));
      if (child) this.track(child);
    }
  }
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
      this.startHelpers();
    } else {
      const id = this.blocker;
      if (id !== null) this.power.stop(id);
      this.blocker = null;
      this.stopHelpers();
    }
    const state = Object.freeze({ active: this.active });
    for (const listener of [...this.listeners]) {
      try { listener(state); } catch { /* Observers cannot roll back the switch or suppress other listeners. */ }
    }
    return this.active;
  }
  dispose(): void { this.set(false); this.listeners.clear(); }
  private windowsState(): string | undefined {
    return this.platform === 'win32' && this.options.stateDir ? path.join(this.options.stateDir, 'keep-awake-lid.txt') : undefined;
  }
  private startHelpers(): void {
    const stateDir = this.options.stateDir, state = this.windowsState();
    const stop = state && stateDir ? path.join(stateDir, `keep-awake-stop-${randomUUID()}`) : undefined;
    this.helpers = helperCommands(this.platform, this.options.pid ?? process.pid, { state, stop }).map(spec => ({ spec, child: null, retry: null, missing: false }));
    for (const helper of this.helpers) this.launch(helper);
  }
  private run(spec: HelperSpec): ChildProcess | null {
    try {
      const child = (this.options.spawn ?? ((command, args) => spawn(command, args, { stdio: ['pipe', 'ignore', 'ignore'], windowsHide: true })))(spec.command, spec.args);
      child.stdin?.on('error', () => {});
      child.on('error', () => {});
      return child;
    } catch { return null; }
  }
  private launch(helper: Helper): void {
    helper.retry = null;
    if (!this.helpers.includes(helper)) return;
    // A previous Windows helper must restore the lid setting before a new one reads it.
    if (this.stopping.size) { this.afterStop.push(() => this.launch(helper)); return; }
    const child = this.run(helper.spec);
    helper.child = child;
    if (!child) { this.scheduleRetry(helper); return; }
    child.on('error', error => {
      if ((error as NodeJS.ErrnoException).code === 'ENOENT') helper.missing = true; // Missing tool: Electron's blocker still applies.
      this.lost(helper, child);
    });
    child.on('exit', () => this.lost(helper, child));
  }
  private lost(helper: Helper, child: ChildProcess): void {
    if (helper.child !== child) return;
    helper.child = null;
    this.scheduleRetry(helper);
  }
  private scheduleRetry(helper: Helper): void {
    // Keep awake must survive a helper crash or a restarted logind/session for the whole night.
    if (helper.missing || helper.retry || !this.helpers.includes(helper)) return;
    helper.retry = setTimeout(() => this.launch(helper), RETRY_MS);
    helper.retry.unref?.();
  }
  /** Waits for a lid-restoring Windows process; it is killed only if it hangs. */
  private track(child: ChildProcess, cleanup?: () => void): void {
    this.stopping.add(child);
    const timer = setTimeout(() => { try { child.kill(); } catch { /* already gone */ } }, STOP_GRACE_MS);
    timer.unref?.();
    let finished = false;
    const done = () => {
      if (finished) return;
      finished = true; clearTimeout(timer); this.stopping.delete(child); cleanup?.();
      if (this.stopping.size) return;
      const queued = this.afterStop; this.afterStop = [];
      for (const run of queued) run();
    };
    child.once('exit', done); child.once('error', done);
  }
  private stopHelpers(): void {
    const helpers = this.helpers; this.helpers = [];
    for (const helper of helpers) {
      if (helper.retry) clearTimeout(helper.retry);
      helper.retry = null;
      const child = helper.child; helper.child = null;
      if (!child) continue;
      try { child.stdin?.end(); } catch { /* already closed */ }
      if (helper.spec.stopFile) {
        // Ask the Windows helper to restore the lid setting itself; kill only if it hangs.
        try { writeFileSync(helper.spec.stopFile, ''); } catch { /* fall back to kill */ }
        if (child.exitCode === null && child.signalCode === null) {
          const stopFile = helper.spec.stopFile;
          this.track(child, () => { try { rmSync(stopFile, { force: true }); } catch { /* best effort */ } });
          continue;
        }
      }
      try { child.kill(); } catch { /* already gone */ }
    }
  }
}
