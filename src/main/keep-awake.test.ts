import { EventEmitter } from 'node:events';
import { mkdtempSync, existsSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ChildProcess } from 'node:child_process';
import { KeepAwake, helperCommands } from './keep-awake';

function fakeChild() {
  const child = new EventEmitter() as EventEmitter & { stdin: EventEmitter & { end: () => void }; kill: () => void; exitCode: number | null; signalCode: string | null };
  child.stdin = Object.assign(new EventEmitter(), { end: vi.fn() }); child.kill = vi.fn(); child.exitCode = null; child.signalCode = null;
  return child;
}
describe('keep awake', () => {
  afterEach(() => { vi.useRealTimers(); });
  it('toggles the power blocker and the Linux idle/sleep and lid-switch inhibitors', () => {
    const power = { start: vi.fn(() => 7), stop: vi.fn() }, children = [fakeChild(), fakeChild()];
    const spawn = vi.fn((_command: string, _args: string[]) => children[spawn.mock.calls.length - 1] as unknown as ChildProcess);
    const keep = new KeepAwake(power, { platform: 'linux', pid: 42, spawn });
    expect(keep.active).toBe(false);
    expect(keep.set(true)).toBe(true); expect(keep.set(true)).toBe(true);
    expect(power.start).toHaveBeenCalledExactlyOnceWith('prevent-display-sleep');
    expect(spawn).toHaveBeenCalledTimes(2);
    expect(spawn.mock.calls.map(([command, args]) => [command, args[0]])).toEqual([['systemd-inhibit', '--what=idle:sleep'], ['systemd-inhibit', '--what=handle-lid-switch']]);
    expect(keep.set(false)).toBe(false); keep.dispose();
    expect(power.stop).toHaveBeenCalledExactlyOnceWith(7);
    for (const child of children) { expect(child.stdin.end).toHaveBeenCalled(); expect(child.kill).toHaveBeenCalledOnce(); }
  });
  it('restarts a helper that dies while keep awake is on, and not after it is off', () => {
    vi.useFakeTimers();
    const spawn = vi.fn((_command: string, _args: string[]) => fakeChild() as unknown as ChildProcess);
    const keep = new KeepAwake({ start: () => 1, stop() {} }, { platform: 'darwin', pid: 5, spawn });
    keep.set(true); expect(spawn).toHaveBeenCalledOnce();
    (spawn.mock.results[0]!.value as EventEmitter).emit('exit', 1);
    vi.advanceTimersByTime(30_000); expect(spawn).toHaveBeenCalledTimes(2);
    (spawn.mock.results[1]!.value as EventEmitter).emit('exit', 1);
    keep.set(false); vi.advanceTimersByTime(60_000); expect(spawn).toHaveBeenCalledTimes(2);
  });
  it('stops the Windows helper through its stop file so it restores the lid action', () => {
    vi.useFakeTimers();
    const dir = mkdtempSync(path.join(tmpdir(), 'shellfox-awake-')), children = [fakeChild(), fakeChild()];
    const spawn = vi.fn((_command: string, _args: string[]) => children[spawn.mock.calls.length - 1] as unknown as ChildProcess);
    const keep = new KeepAwake({ start: () => 1, stop() {} }, { platform: 'win32', pid: 3, spawn, stateDir: dir });
    keep.set(true);
    const script = Buffer.from(spawn.mock.calls[0]![1].at(-1)!, 'base64').toString('utf16le');
    const stop = /-Stop '([^']+)'/.exec(script)![1]!;
    expect(script).toContain(`-State '${path.join(dir, 'keep-awake-lid.txt')}'`); expect(script).toContain('LIDACTION');
    keep.set(false);
    expect(existsSync(stop)).toBe(true); expect(children[0]!.kill).not.toHaveBeenCalled();
    keep.set(true); expect(spawn).toHaveBeenCalledOnce(); // Waits for the restore before reading the lid action again.
    children[0]!.emit('exit', 0);
    expect(existsSync(stop)).toBe(false); expect(spawn).toHaveBeenCalledTimes(2);
    keep.set(false); vi.advanceTimersByTime(10_000); expect(children[1]!.kill).toHaveBeenCalledOnce();
  });
  it('restores a lid action left behind by a hard-killed Windows helper', () => {
    const dir = mkdtempSync(path.join(tmpdir(), 'shellfox-awake-'));
    writeFileSync(path.join(dir, 'keep-awake-lid.txt'), '1,1');
    const spawn = vi.fn((_command: string, _args: string[]) => fakeChild() as unknown as ChildProcess);
    new KeepAwake({ start: () => 1, stop() {} }, { platform: 'win32', pid: 3, spawn, stateDir: dir });
    expect(spawn).toHaveBeenCalledOnce();
    expect(Buffer.from(spawn.mock.calls[0]![1].at(-1)!, 'base64').toString('utf16le')).toContain('-RestoreOnly');
  });
  it('keeps the Electron blocker when the helper tool is missing', () => {
    vi.useFakeTimers();
    const power = { start: vi.fn(() => 1), stop: vi.fn() }, child = fakeChild();
    const spawn = vi.fn(() => child as unknown as ChildProcess);
    const keep = new KeepAwake(power, { platform: 'darwin', pid: 1, spawn });
    keep.set(true); child.emit('error', Object.assign(new Error('spawn caffeinate ENOENT'), { code: 'ENOENT' }));
    vi.advanceTimersByTime(60_000); expect(spawn).toHaveBeenCalledOnce();
    expect(keep.active).toBe(true); keep.set(false);
    expect(child.kill).not.toHaveBeenCalled(); expect(power.stop).toHaveBeenCalledOnce();
  });
  it('notifies real state changes, supports unsubscription and clears observers on disposal', () => {
    const keep = new KeepAwake({ start: () => 1, stop() {} }, { platform: 'freebsd' as NodeJS.Platform });
    const observed = vi.fn(), stop = keep.subscribe(observed);
    keep.set(true); keep.set(true); keep.set(false); keep.set(false);
    expect(observed.mock.calls.map(([state]) => state)).toEqual([{ active: true }, { active: false }]);
    expect(Object.isFrozen(observed.mock.calls[0]![0])).toBe(true);
    stop(); stop(); keep.set(true); expect(observed).toHaveBeenCalledTimes(2);
    const final = vi.fn(); keep.subscribe(final); keep.dispose(); keep.dispose();
    expect(final).toHaveBeenCalledExactlyOnceWith({ active: false });
    keep.set(true); expect(final).toHaveBeenCalledOnce(); keep.dispose();
  });
  it('isolates observer failures without failing a toggle or suppressing later observers', () => {
    const keep = new KeepAwake({ start: () => 1, stop() {} }, { platform: 'freebsd' as NodeJS.Platform });
    const observer = vi.fn();
    keep.subscribe(() => { throw new Error('Renderer gone.'); }); keep.subscribe(observer);
    expect(keep.set(true)).toBe(true); expect(observer).toHaveBeenCalledExactlyOnceWith({ active: true });
    keep.dispose(); expect(observer).toHaveBeenLastCalledWith({ active: false });
  });
  it('does not notify a failed blocker change and retains the handle for retry', () => {
    const power = { start: vi.fn(() => 7), stop: vi.fn() };
    const keep = new KeepAwake(power, { platform: 'freebsd' as NodeJS.Platform }), observer = vi.fn();
    keep.subscribe(observer);
    power.start.mockImplementationOnce(() => { throw new Error('Start failed.'); });
    expect(() => keep.set(true)).toThrow('Start failed'); expect(keep.active).toBe(false); expect(observer).not.toHaveBeenCalled();
    keep.set(true); observer.mockClear();
    power.stop.mockImplementationOnce(() => { throw new Error('Stop failed.'); });
    expect(() => keep.set(false)).toThrow('Stop failed'); expect(keep.active).toBe(true); expect(observer).not.toHaveBeenCalled();
    keep.set(false); expect(power.stop).toHaveBeenLastCalledWith(7); expect(observer).toHaveBeenCalledExactlyOnceWith({ active: false });
  });
  it('builds helpers that die with Shellfox', () => {
    expect(helperCommands('darwin', 9)).toEqual([{ command: 'caffeinate', args: ['-dimsu', '-w', '9'] }]);
    expect(helperCommands('linux', 9).map(spec => spec.args.at(-1))).toEqual(['cat', 'cat']);
    const win = helperCommands('win32', 9)[0]!;
    expect(win.command).toBe('powershell.exe');
    const script = Buffer.from(win.args.at(-1)!, 'base64').toString('utf16le');
    expect(script).toContain('-Owner 9'); expect(script).toContain('Get-Process -Id $Owner');
    expect(helperCommands('freebsd' as NodeJS.Platform, 9)).toEqual([]);
  });
});
