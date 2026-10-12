import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { execFile } from 'node:child_process';
import { open } from 'node:fs/promises';
import { createSystemSnapshotProvider, type TrackingSnapshot } from './terminal/tracking';
import { collectSystemMetrics } from './control-stats-metrics';

vi.mock('node:child_process', () => ({ execFile: vi.fn() }));
vi.mock('node:fs/promises', async original => ({ ...await original<typeof import('node:fs/promises')>(), open: vi.fn() }));
vi.mock('./terminal/tracking', async original => ({ ...await original<typeof import('./terminal/tracking')>(), createSystemSnapshotProvider: vi.fn() }));
const platform = Object.getOwnPropertyDescriptor(process, 'platform')!;
const boot = '12345678-1234-1234-1234-123456789abc';
const snap = (overrides: Partial<TrackingSnapshot> = {}): TrackingSnapshot => ({
  domain: 'host', environment: 'local', distro: null, complete: true,
  processes: [{ pid: 10, parentPid: 1, birth: boot + ':1000', birthOrder: '1000', executable: '/bin/bash', argv: ['bash'], accessible: true }], ...overrides,
});
const stat = (ticks = '1000', parent = '1') => '10 (bash) ' + ['S', parent, ...Array(17).fill('0'), ticks, '1000000', '10'].join(' ');
let replies: (file: string, args: string[]) => string;
let files: Record<string, string>;
const closes = vi.fn();
beforeEach(() => {
  files = {
    '/proc/sys/kernel/random/boot_id': boot,
    '/proc/uptime': '100.00 20.00',
    '/proc/10/stat': stat(),
    '/proc/10/status': 'VmRSS: 42 kB\n',
  };
  replies = () => '100\n';
  vi.mocked(execFile).mockImplementation(((file: string, args: string[], _options: unknown, callback: (error: Error | null, stdout: string) => void) => {
    try { callback(null, replies(file, args)); } catch (error) { callback(error as Error, ''); }
  }) as typeof execFile);
  vi.mocked(open).mockImplementation((async (path: string) => {
    const raw = files[path];
    if (raw === undefined) throw new Error('missing');
    const source = Buffer.from(raw); let offset = 0;
    return {
      async read(buffer: Buffer, target: number, length: number) {
        const bytesRead = source.copy(buffer, target, offset, offset + length); offset += bytesRead; return { bytesRead, buffer };
      }, close: closes,
    };
  }) as unknown as typeof open);
});
afterEach(() => { Object.defineProperty(process, 'platform', platform); vi.resetAllMocks(); });
const setPlatform = (value: NodeJS.Platform) => Object.defineProperty(process, 'platform', { value, configurable: true });
const signal = () => AbortSignal.timeout(20_000);

describe('bounded system metric collection', () => {
  it('reads Linux RSS and runtime without launching per-PID commands', async () => {
    setPlatform('linux');
    const [domain] = await collectSystemMetrics([snap()], [], signal());
    expect(domain.processes).toEqual([{ pid: 10, parentPid: 1, birth: boot + ':1000', memoryBytes: 42 * 1024, elapsedSeconds: 90 }]);
    expect(execFile).toHaveBeenCalledTimes(1);
    expect(vi.mocked(execFile).mock.calls[0].slice(0, 2)).toEqual(['/usr/bin/getconf', ['CLK_TCK']]);
    expect(closes).toHaveBeenCalledTimes(5);
  });
  it('keeps Linux RSS when CLK_TCK is unavailable rather than guessing runtime', async () => {
    setPlatform('linux'); replies = () => { throw new Error('no getconf'); };
    const [domain] = await collectSystemMetrics([snap()], [], signal());
    expect(domain.processes[0]).toMatchObject({ memoryBytes: 42 * 1024, elapsedSeconds: null });
  });
  it('rejects Linux metrics when PID birth changes during measurement', async () => {
    setPlatform('linux');
    const original = vi.mocked(open).getMockImplementation()!;
    let reads = 0;
    vi.mocked(open).mockImplementation(((path: string, ...args: unknown[]) => {
      if (path === '/proc/10/stat' && ++reads === 2) files[path] = stat('2000');
      return Reflect.apply(original, null, [path, ...args]);
    }) as typeof open);
    expect((await collectSystemMetrics([snap()], [], signal()))[0].processes).toEqual([]);
  });
  it('rejects Linux metrics from another boot or changed parent', async () => {
    setPlatform('linux');
    expect((await collectSystemMetrics([snap({ processes: [{ ...snap().processes[0], birth: 'other:1000' }] })], [], signal()))[0].processes).toEqual([]);
    files['/proc/10/stat'] = stat('1000', '2');
    expect((await collectSystemMetrics([snap()], [], signal()))[0].processes).toEqual([]);
  });
  it('batches Windows CIM once and passes a malicious-looking WSL distro only as an argument', async () => {
    setPlatform('win32');
    replies = () => JSON.stringify([{ pid: 10, parentPid: 1, birth: '133000000000000001', memoryBytes: 1024, elapsedSeconds: 12 }]);
    const distro = 'Ubuntu; $(touch injected)';
    const results = await collectSystemMetrics([snap(), snap({ domain: 'guest', environment: 'wsl', distro })], [], signal());
    expect(results).toHaveLength(2); expect(execFile).toHaveBeenCalledTimes(2);
    const calls = vi.mocked(execFile).mock.calls;
    const host = calls.find(call => String(call[0]).endsWith('powershell.exe'))!;
    const guest = calls.find(call => String(call[0]).endsWith('wsl.exe'))!;
    const hostArgs = host[1] as string[], guestArgs = guest[1] as string[];
    expect(hostArgs.slice(0, 4)).toEqual(['-NoLogo', '-NoProfile', '-NonInteractive', '-Command']);
    expect(hostArgs[4]).toContain('Get-CimInstance Win32_Process');
    expect(hostArgs[4]).toContain('ToFileTimeUtc');
    expect(guestArgs.slice(0, 6)).toEqual(['-d', distro, '--exec', 'python3', '-c', guestArgs[5]]);
    expect(guestArgs[5]).not.toContain(distro);
    for (const call of calls) expect(call[2]).toMatchObject({ timeout: 8000, maxBuffer: 8 * 1024 * 1024, windowsHide: true, signal: expect.any(AbortSignal) });
  });
  it('reports unavailable WSL Python metrics instead of shelling out per process', async () => {
    setPlatform('win32'); replies = () => { throw new Error('python not found'); };
    const [domain] = await collectSystemMetrics([snap({ environment: 'wsl', distro: 'Ubuntu' })], [], signal());
    expect(domain.processes).toEqual([]); expect(domain.reason).toContain('Python 3'); expect(execFile).toHaveBeenCalledTimes(1);
  });
  it('brackets one macOS ps command with fresh exact-birth evidence', async () => {
    setPlatform('darwin'); replies = () => '10 1 12\n';
    const s = snap({ processes: [{ ...snap().processes[0], birth: 'darwin:1000:000001' }] });
    const provider = vi.fn(async () => [s]); vi.mocked(createSystemSnapshotProvider).mockReturnValue(provider);
    const [domain] = await collectSystemMetrics([s], [], signal());
    expect(domain.processes[0].memoryBytes).toBe(12 * 1024);
    expect(execFile).toHaveBeenCalledTimes(1); expect(provider).toHaveBeenCalledTimes(1);
    expect(vi.mocked(execFile).mock.calls[0].slice(0, 2)).toEqual(['/bin/ps', ['-axo', 'pid=,ppid=,rss=']]);
  });
  it('reports unsupported platforms and command failures without fake zeros', async () => {
    setPlatform('freebsd');
    expect((await collectSystemMetrics([snap()], [], signal()))[0]).toMatchObject({ processes: [], reason: expect.any(String) });
    setPlatform('win32'); replies = () => { throw new Error('timeout'); };
    expect((await collectSystemMetrics([snap()], [], signal()))[0]).toMatchObject({ processes: [], reason: expect.any(String) });
  });
});
