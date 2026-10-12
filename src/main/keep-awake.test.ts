import { EventEmitter } from 'node:events';
import { describe, expect, it, vi } from 'vitest';
import type { ChildProcess } from 'node:child_process';
import { KeepAwake, helperCommand } from './keep-awake';

function fakeChild() {
  const child = new EventEmitter() as EventEmitter & { stdin: EventEmitter & { end: () => void }; kill: () => void };
  child.stdin = Object.assign(new EventEmitter(), { end: vi.fn() }); child.kill = vi.fn();
  return child;
}
describe('keep awake', () => {
  it('toggles the power blocker and one OS helper', () => {
    const power = { start: vi.fn(() => 7), stop: vi.fn() }, child = fakeChild();
    const spawn = vi.fn((_command: string, _args: string[]) => child as unknown as ChildProcess);
    const keep = new KeepAwake(power, { platform: 'linux', pid: 42, spawn });
    expect(keep.active).toBe(false);
    expect(keep.set(true)).toBe(true); expect(keep.set(true)).toBe(true);
    expect(power.start).toHaveBeenCalledExactlyOnceWith('prevent-display-sleep');
    expect(spawn).toHaveBeenCalledOnce(); expect(spawn.mock.calls[0][0]).toBe('systemd-inhibit');
    expect(keep.set(false)).toBe(false); keep.dispose();
    expect(power.stop).toHaveBeenCalledExactlyOnceWith(7);
    expect(child.stdin.end).toHaveBeenCalled(); expect(child.kill).toHaveBeenCalledOnce();
  });
  it('keeps the Electron blocker when the helper tool is missing', () => {
    const power = { start: vi.fn(() => 1), stop: vi.fn() }, child = fakeChild();
    const keep = new KeepAwake(power, { platform: 'darwin', pid: 1, spawn: () => child as unknown as ChildProcess });
    keep.set(true); child.emit('error', new Error('ENOENT'));
    expect(keep.active).toBe(true); keep.set(false);
    expect(child.kill).not.toHaveBeenCalled(); expect(power.stop).toHaveBeenCalledOnce();
  });
  it('builds helpers that die with Shellfox', () => {
    expect(helperCommand('darwin', 9)).toEqual({ command: 'caffeinate', args: ['-dimsu', '-w', '9'] });
    expect(helperCommand('linux', 9)?.args.at(-1)).toBe('cat');
    const win = helperCommand('win32', 9)!;
    expect(win.command).toBe('powershell.exe');
    const script = Buffer.from(win.args.at(-1)!, 'base64').toString('utf16le');
    expect(script).toContain('-Owner 9'); expect(script).toContain('Get-Process -Id $Owner');
    expect(helperCommand('freebsd' as NodeJS.Platform, 9)).toBeNull();
  });
});
