import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import type { ChildProcessWithoutNullStreams } from 'node:child_process';
import { afterEach, expect, it, vi } from 'vitest';
const mocks = vi.hoisted(() => ({ spawn: vi.fn() }));
vi.mock('node:child_process', async original => ({ ...await original<typeof import('node:child_process')>(), spawn: mocks.spawn }));
import { WindowsElevatedPty, elevatedHelperPath, launchElevatedPty, elevatedLauncherEnvironment } from './windows-elevated';
const processes: { kill(): void }[] = [];
const children: EventEmitter[] = [];
function fixture() {
  const child = Object.assign(new EventEmitter(), { pid: 123, stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough() });
  children.push(child);
  const sent: string[] = []; child.stdin.on('data', data => sent.push(data.toString()));
  const pty = new WindowsElevatedPty(child as unknown as ChildProcessWithoutNullStreams, { cwd: 'C:\\work', env: {}, cols: 80, rows: 24 }); processes.push(pty);
  const message = (value: object) => child.stdout.write(JSON.stringify(value) + '\n');
  const ready = () => message({ type: 'ready', pid: 456, birth: '133000000000000000' });
  return { child, pty, message, sent, ready };
}
afterEach(() => { for (const pty of processes.splice(0)) pty.kill(); for (const child of children.splice(0)) child.emit('close', 1); vi.clearAllMocks(); });
it('uses private stdio, not Electron elevation, shell interpolation or an external window', () => {
  const f = fixture(); mocks.spawn.mockReturnValue(f.child);
  const pty = launchElevatedPty({ cwd: "C:\\a';& folder", env: { TEST: 'a\nb' }, cols: 80, rows: 24 }); processes.push(pty);
  expect(mocks.spawn).toHaveBeenCalledWith(expect.stringMatching(/Shellfox.ElevatedTerminal.exe$/), ['launch'], expect.objectContaining({ windowsHide: true, stdio: 'pipe', shell: false, env: expect.any(Object) }));
  expect(JSON.parse(f.sent[1])).toMatchObject({ type: 'launch', cwd: "C:\\a';& folder", env: { TEST: 'a\nb' } });
  f.message({ type: 'exit', exitCode: 1 });
});
it('rejects oversized launch before spawning a process', () => {
  expect(() => launchElevatedPty({ cwd: 'C:\\work', env: { TOO_BIG: 'x'.repeat(262144) }, cols: 80, rows: 24 })).toThrow('too large');
  expect(mocks.spawn).not.toHaveBeenCalled();
});
it('authenticates readiness and preserves UTF-8 split across output frames', async () => {
  const f = fixture(), output: string[] = []; f.pty.onData(data => output.push(data));
  expect(f.pty.queuedInputBytes()).toBeNull(); f.ready(); await f.pty.whenReady();
  expect(f.pty.pid).toBe(456); expect(f.pty.birth).toBe('133000000000000000');
  const utf8 = Buffer.from('雪🙂\x1b[31m');
  for (const data of [utf8.subarray(0, 2), utf8.subarray(2, 5), utf8.subarray(5)]) f.message({ type: 'data', data: data.toString('base64') });
  expect(output.join('')).toBe('雪🙂\x1b[31m');
  f.message({ type: 'exit', exitCode: 17 }); await f.pty.closeConfirmed();
});
it('bounds outstanding input until exact ordered acknowledgements arrive', async () => {
  const f = fixture(); f.ready(); await f.pty.whenReady();
  f.pty.write('x'.repeat(65536)); f.pty.write('y'.repeat(65536));
  expect(f.pty.queuedInputBytes()).toBe(131072); expect(() => f.pty.write('z')).toThrow();
  f.message({ type: 'ack', id: 1 }); expect(f.pty.queuedInputBytes()).toBe(65536);
  f.pty.write('z'); f.message({ type: 'ack', id: 2 }); f.message({ type: 'ack', id: 3 });
  expect(f.pty.queuedInputBytes()).toBe(0); f.message({ type: 'exit', exitCode: 0 });
});
it('accepts a 50k-apostrophe write and rejects escaped oversize before enqueue without disconnecting', async () => {
  const f = fixture(); f.ready(); await f.pty.whenReady();
  const count = f.sent.length;
  f.pty.write("'".repeat(50000));
  expect(Buffer.byteLength(f.sent[count])).toBeLessThan(262144);
  expect(JSON.parse(f.sent[count])).toEqual({ type: 'write', id: 1, data: "'".repeat(50000) });
  f.message({ type: 'ack', id: 1 });
  expect(() => f.pty.write('\x01'.repeat(50000))).toThrow('oversized');
  expect(f.pty.queuedInputBytes()).toBe(0); expect(f.sent).toHaveLength(count + 1);
  expect(f.child.stdin.destroyed).toBe(false); expect(f.child.stdout.destroyed).toBe(false);
  f.pty.write('still usable'); expect(JSON.parse(f.sent.at(-1)!)).toMatchObject({ type: 'write', id: 2 });
  f.message({ type: 'ack', id: 2 }); f.message({ type: 'exit', exitCode: 0 }); await f.pty.closeConfirmed();
});
it('accepts the shared 500-row bound and rejects rows outside 2..500', async () => {
  const f = fixture(); f.ready(); await f.pty.whenReady();
  f.pty.resize(80, 500); expect(JSON.parse(f.sent.at(-1)!)).toEqual({ type: 'resize', cols: 80, rows: 500 });
  for (const rows of [1, 501]) expect(() => f.pty.resize(80, rows)).toThrow();
  f.message({ type: 'exit', exitCode: 0 });
});
it('rejects bad ack, oversized frames and arbitrary protocol fields without claiming cleanup', async () => {
  for (const bad of [{ type: 'ack', id: 99 }, { type: 'ready', pid: 789, birth: '123' }, { type: 'data', data: 'eA==', command: 'evil' }]) {
    const f = fixture(); f.ready(); await f.pty.whenReady(); f.message(bad);
    expect(f.child.stdin.destroyed).toBe(true); f.child.emit('close', 1);
    await expect(f.pty.closeConfirmed()).rejects.toThrow('protocol');
  }
  const f = fixture(); f.child.stdout.write('x'.repeat(262145));
  expect(f.child.stdin.destroyed).toBe(true); f.child.emit('close', 1);
  await expect(f.pty.whenReady()).rejects.toThrow('protocol');
});
it('accepts an in-flight ready after cancellation and keeps reading cleanup evidence', async () => {
  const f = fixture(); const closed = f.pty.closeConfirmed();
  expect(JSON.parse(f.sent[1])).toEqual({ type: 'close' });
  f.ready(); expect(f.child.stdout.destroyed).toBe(false);
  f.message({ type: 'exit', exitCode: 1 }); await closed;
  await expect(f.pty.whenReady()).rejects.toThrow('cancelled');
});
it('UAC denial is a failed launch but can confirm no shell was started', async () => {
  const f = fixture(), exits = vi.fn(); f.pty.onExit(exits);
  f.message({ type: 'error', code: 'cancelled', message: 'Administrator PowerShell was cancelled or timed out.' });
  f.message({ type: 'exit', exitCode: 1 });
  await expect(f.pty.whenReady()).rejects.toThrow('cancelled'); await f.pty.closeConfirmed();
  expect(exits).toHaveBeenCalledWith({ exitCode: 1, signal: 0 });
});
it('stops input after post-ready failure and does not confuse launcher death with job cleanup', async () => {
  const f = fixture(), exits = vi.fn(); f.pty.onExit(exits); f.ready(); await f.pty.whenReady();
  f.message({ type: 'error', code: 'failed', message: 'Helper disconnected.' });
  expect(f.pty.queuedInputBytes()).toBeNull(); expect(() => f.pty.write('x')).toThrow(); expect(() => f.pty.resize(100, 40)).toThrow();
  f.child.emit('close', 1); await expect(f.pty.closeConfirmed()).rejects.toThrow();
  expect(exits).toHaveBeenCalledWith({ exitCode: 1, signal: 1 });
});
it('strips CLR injection variables case-insensitively', () => {
  expect(elevatedLauncherEnvironment({ Path: 'C:\\\\Windows', CUSTOM: 'ok', DOTNET_STARTUP_HOOKS: 'evil', ComPlus_EnableDiagnostics: '1', CORECLR_PROFILER_PATH: 'evil', COR_ENABLE_PROFILING: '1' })).toEqual({ Path: 'C:\\\\Windows', CUSTOM: 'ok' });
});
it('removes session and inherited CLR injection switches from the final shell spec', () => {
  const f = fixture(); mocks.spawn.mockReturnValue(f.child);
  const pty = launchElevatedPty({ cwd: 'C:\\\\work', env: { CUSTOM: 'kept', COR_ENABLE_PROFILING: '1', cor_profiler: 'evil', CoR_PROFILER_PATH_64: 'evil', CORECLR_ENABLE_PROFILING: '1', DOTNET_STARTUP_HOOKS: 'evil', COMPLUS_Version: 'evil' }, cols: 80, rows: 24 }); processes.push(pty);
  expect(JSON.parse(f.sent.at(-1)!)).toMatchObject({ type: 'launch', env: { CUSTOM: 'kept' } });
  expect(Object.keys(JSON.parse(f.sent.at(-1)!).env)).toEqual(['CUSTOM']);
  f.message({ type: 'exit', exitCode: 1 });
});
it('resolves packaged helper outside ASAR and development helper for target architecture', () => {
  expect(elevatedHelperPath({ resourcesPath: 'C:\\Shellfox\\resources', packaged: true })).toMatch(/elevated-terminal[\\/]Shellfox.ElevatedTerminal.exe$/);
  expect(elevatedHelperPath({ packaged: false, arch: 'arm64' })).toMatch(/win-arm64[\\/]Shellfox.ElevatedTerminal.exe$/);
});
