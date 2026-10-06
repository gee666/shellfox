import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { z } from 'zod';
import { UnsupportedBackend, WindowsBackend } from './index';
import { BrokerTransport, MAX_FRAME } from './transport';
import type { NativeEvent, NativeInit } from '../../shared/native-port';

vi.mock('node:child_process', () => ({ spawn: vi.fn() }));
const init: NativeInit = { userDataDir: 'C:\\tmp\\user', helperDir: 'C:\\tmp\\native', shellScriptDir: 'C:\\app\\shell', packagedExecutable: null };
class FakeChild extends EventEmitter {
  stdin = new PassThrough(); stdout = new PassThrough(); stderr = new PassThrough(); exitCode: number | null = null;
  kill = vi.fn(() => { this.exitCode = 0; this.emit('exit', 0); return true; });
}
let child: FakeChild;
beforeEach(() => { child = new FakeChild(); vi.mocked(spawn).mockReturnValue(child as unknown as ChildProcessWithoutNullStreams); });
afterEach(() => { vi.useRealTimers(); vi.clearAllMocks(); });
const frame = (value: unknown) => Buffer.from(JSON.stringify(value) + '\n');
function sent(): { id: string; method: string } { return JSON.parse(child.stdin.read().toString()); }

describe('unsupported platform', () => {
  it('disables every capability and never launches', async () => {
    const backend = new UnsupportedBackend(); const result = await backend.initialize(init);
    expect(result.ok).toBe(true);
    if (result.ok) { expect(result.value.available).toBe(false); expect(Object.values(result.value.capabilities).every(v => v === false)).toBe(true); }
    expect(await backend.setExplorerIntegration({ installed: true, executablePath: 'C:\\app.exe' })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
    expect(await backend.setWatch({ tabs: [], rules: [] })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
    expect(spawn).not.toHaveBeenCalled();
  });
  it('refuses an existing-window append without starting or contacting a helper', async () => {
    const backend = new WindowsBackend();
    const id = '00000000-0000-4000-8000-000000000001';
    const target = { kind: 'windows-terminal' as const, windowName: `shellfox-${id}`, hwnd: '123', owner: { pid: 1, startTime: '100' }, sessionId: id, markerPrefix: `SHELLFOX:${id}:`, verification: 'native-title' as const };
    const result = await backend.launch({ sessionId: id, tabId: id, operationId: id, cwd: 'C:\\tmp', shellId: 'pwsh', shellExecutable: 'C:\\pwsh.exe', windowName: `shellfox-${id}`, titleMarker: `SHELLFOX:${id}:${id}`, existingTarget: target });
    expect(result).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
    expect(spawn).not.toHaveBeenCalled();
  });
  it('rejects malformed generated launch fields before transport', async () => {
    const backend = new WindowsBackend();
    const id = '00000000-0000-4000-8000-000000000001';
    expect(await backend.launch({ sessionId: id, tabId: id, operationId: id, cwd: 'C:\\tmp', shellId: 'pwsh', shellExecutable: 'C:\\pwsh.exe', windowName: 'bad;new-tab', titleMarker: 'evil', existingTarget: null })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
    expect(spawn).not.toHaveBeenCalled();
  });
});

describe('private helper transport', () => {
  it('spawns shell:false and accepts only correlated validated results', async () => {
    const events: NativeEvent[] = []; const transport = new BrokerTransport(e => events.push(e));
    transport.start('C:\\Shellfox.Native.exe');
    expect(spawn).toHaveBeenCalledWith('C:\\Shellfox.Native.exe', ['broker'], expect.objectContaining({ shell: false, windowsHide: true }));
    const promise = transport.request('test', {}, z.object({ configured: z.literal(true) }).strict());
    const request = sent();
    const reply = frame({ version: 1, kind: 'response', id: request.id, result: { ok: true, value: { configured: true } } });
    transport.accept(reply.subarray(0, 12)); transport.accept(reply.subarray(12));
    expect(await promise).toEqual({ ok: true, value: { configured: true } }); expect(events).toHaveLength(0);
    child.exitCode = 0; await transport.dispose();
  });
  it('rejects protocol versions, foreign replies and unknown DTO fields', async () => {
    for (const kind of ['version', 'id', 'dto']) {
      const events: NativeEvent[] = []; const transport = new BrokerTransport(e => events.push(e)); transport.start('C:\\helper.exe');
      const promise = transport.request('test', {}, z.object({ configured: z.literal(true) }).strict()); const request = sent();
      transport.accept(frame({ version: kind === 'version' ? 2 : 1, kind: 'response', id: kind === 'id' ? '00000000-0000-4000-8000-000000000001' : request.id, result: { ok: true, value: kind === 'dto' ? { configured: true, rawToken: 'not-accepted' } : { configured: true } } }));
      expect(await promise).toMatchObject({ ok: false, error: { code: 'NATIVE_UNAVAILABLE' } });
      expect(events.at(-1)?.type).toBe('unavailable');
      expect(child.kill).toHaveBeenCalled();
      child = new FakeChild(); vi.mocked(spawn).mockReturnValue(child as unknown as ChildProcessWithoutNullStreams);
    }
  });
  it('bounds frames and rejects invalid UTF-8', () => {
    for (const bytes of [Buffer.alloc(MAX_FRAME + 1), Buffer.from([0xff, 10])]) {
      const events: NativeEvent[] = []; const transport = new BrokerTransport(e => events.push(e)); transport.start('C:\\helper.exe'); transport.accept(bytes);
      expect(events).toHaveLength(1); expect(events[0].type).toBe('unavailable');
      child = new FakeChild(); vi.mocked(spawn).mockReturnValue(child as unknown as ChildProcessWithoutNullStreams);
    }
  });
  it('helper exit rejects requests and never relaunches it', async () => {
    const events: NativeEvent[] = []; const transport = new BrokerTransport(e => events.push(e)); transport.start('C:\\helper.exe');
    const pending = transport.request('test', {}, z.object({}).strict()); child.emit('exit', 1);
    expect(await pending).toMatchObject({ ok: false, error: { code: 'NATIVE_UNAVAILABLE' } }); expect(events.at(-1)?.type).toBe('unavailable'); expect(spawn).toHaveBeenCalledTimes(1);
  });
  it('timeout reports uncertainty rather than retrying launch', async () => {
    vi.useFakeTimers(); const transport = new BrokerTransport(() => {}); transport.start('C:\\helper.exe');
    const pending = transport.request('launch', {}, z.object({}).strict()); await vi.advanceTimersByTimeAsync(10_000);
    expect(await pending).toMatchObject({ ok: false, error: { code: 'NATIVE_UNAVAILABLE', message: expect.stringContaining('may already exist') } }); expect(spawn).toHaveBeenCalledTimes(1);
  });
  it('unavailable events cannot contain unexpected secrets', () => {
    const events: NativeEvent[] = []; const transport = new BrokerTransport(e => events.push(e)); transport.start('C:\\helper.exe');
    transport.accept(frame({ version: 1, kind: 'event', event: { type: 'unavailable', error: { code: 'NATIVE_UNAVAILABLE', message: 'Disconnected', retryable: false }, token: 'rejected' } }));
    expect(events).toHaveLength(1); expect(JSON.stringify(events)).not.toContain('rejected');
  });
});
