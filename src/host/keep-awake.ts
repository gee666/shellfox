import { spawn, type ChildProcess } from 'node:child_process';
import { access } from 'node:fs/promises';
import { constants } from 'node:fs';
import path from 'node:path';
import type { KeepAwakeState } from '../shared/contracts';
import type { HostKeepAwake } from './operations';

/** OS inhibitors only. No Electron blocker and no claim to override lid or lock policies. */
export class HeadlessKeepAwake implements HostKeepAwake {
  private child?: ChildProcess;
  private enabled = false;
  private available = false;
  private readonly listeners = new Set<(state: KeepAwakeState) => void>();
  private queue: Promise<unknown> = Promise.resolve();
  private readonly command = process.platform === 'darwin' ? '/usr/bin/caffeinate' : process.platform === 'linux' ? '/usr/bin/systemd-inhibit' : path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/WindowsPowerShell/v1.0/powershell.exe');
  get active(): boolean { return this.enabled; }
  get supported(): boolean { return this.available; }
  async initialize(): Promise<void> {
    if (!['win32', 'linux', 'darwin'].includes(process.platform)) return;
    try { await access(this.command, process.platform === 'win32' ? constants.F_OK : constants.X_OK); this.available = true; } catch { this.available = false; }
  }
  subscribe(listener: (state: KeepAwakeState) => void): () => void { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; }
  private changed(active: boolean): void { this.enabled = active; for (const listener of this.listeners) { try { listener({ active }); } catch { /* Observers cannot change the inhibitor state. */ } } }
  set(active: boolean): Promise<boolean> {
    const operation = this.queue.then(() => this.apply(active)); this.queue = operation.catch(() => {}); return operation;
  }
  private async apply(active: boolean): Promise<boolean> {
    if (active === this.enabled) return active;
    if (!active) { await this.stop(); return false; }
    if (!this.available) throw new Error('The OS keep-awake helper is unavailable.');
    let args: string[];
    if (process.platform === 'darwin') args = ['-dimsu', '-w', String(process.pid)];
    else if (process.platform === 'linux') args = ['--what=idle:sleep', '--who=Shellfox', '--why=Headless terminals', '--mode=block', '/bin/sh', '-c', 'printf "ready\\n"; exec cat'];
    else {
      const script = `$ErrorActionPreference='Stop'; Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class Awake { [DllImport("kernel32.dll")] public static extern uint SetThreadExecutionState(uint flags); }'; if ([Awake]::SetThreadExecutionState([uint32]2147483651) -eq 0) { throw 'Execution state failed' }; [Console]::Out.WriteLine('ready'); try { [Console]::In.ReadToEnd() | Out-Null } finally { [Awake]::SetThreadExecutionState([uint32]2147483648) | Out-Null }`;
      args = ['-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(script, 'utf16le').toString('base64')];
    }
    const child = spawn(this.command, args, { stdio: ['pipe', 'pipe', 'ignore'], windowsHide: true }); this.child = child;
    child.stdin?.on('error', () => {});
    child.on('error', () => { if (this.child === child) this.changed(false); });
    child.on('exit', () => { if (this.child === child) { this.child = undefined; this.changed(false); } });
    try {
      await new Promise<void>((resolve, reject) => {
        const ready = () => { clearTimeout(timer); child.removeListener('error', failed); child.removeListener('exit', exited); resolve(); };
        const failed = (error: Error) => { clearTimeout(timer); reject(error); };
        const exited = () => failed(new Error('OS sleep inhibition was refused.'));
        child.once('error', failed); child.once('exit', exited);
        const timer = setTimeout(process.platform === 'darwin' ? ready : () => failed(new Error('OS sleep inhibition timed out.')), process.platform === 'darwin' ? 300 : 10_000);
        let output = '';
        if (process.platform !== 'darwin') child.stdout?.on('data', (chunk: Buffer) => { output += chunk.toString('utf8'); if (output.trim() === 'ready') ready(); else if (output.length > 1024) failed(new Error('Invalid OS inhibitor acknowledgement.')); });
      });
      this.changed(true); return true;
    } catch (error) { await this.stop(); throw error; }
  }
  private async stop(): Promise<void> {
    const child = this.child; this.child = undefined; this.changed(false);
    if (!child || child.pid === undefined || child.exitCode !== null || child.signalCode !== null) return;
    await new Promise<void>(resolve => {
      const timer = setTimeout(() => child.kill(), 1500);
      child.once('close', () => { clearTimeout(timer); resolve(); });
      child.stdin?.end();
      if (process.platform === 'darwin') child.kill('SIGTERM');
    });
  }
  async dispose(): Promise<void> { await this.set(false); this.listeners.clear(); }
}
