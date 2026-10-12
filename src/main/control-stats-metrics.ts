import { execFile } from 'node:child_process';
import { open } from 'node:fs/promises';
import { win32 } from 'node:path';
import { createSystemSnapshotProvider, type TrackingSnapshot, type TrackingRoot } from './terminal/tracking';

export interface ProcessMetric {
  pid: number;
  parentPid: number;
  birth: string;
  memoryBytes: number | null;
  elapsedSeconds: number | null;
}
export interface DomainMetrics { domain: string; processes: ProcessMetric[]; reason?: string }
export type MetricsProvider = (snapshots: readonly TrackingSnapshot[], roots: readonly TrackingRoot[], signal: AbortSignal) => Promise<DomainMetrics[]>;
const MAX_ROWS = 20_000;
const MAX_OUTPUT = 8 * 1024 * 1024;
const nonnegative = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v) && v >= 0;
const bytes = (v: unknown): number | null => nonnegative(v) && Number.isSafeInteger(v) ? v : null;

function command(file: string, args: string[], signal: AbortSignal): Promise<string> {
  return new Promise((resolve, reject) => execFile(file, args, {
    timeout: 8000, maxBuffer: MAX_OUTPUT, encoding: 'utf8', windowsHide: true, signal,
  }, (error, stdout) => error ? reject(error) : resolve(stdout)));
}
async function read(path: string, signal: AbortSignal): Promise<string> {
  signal.throwIfAborted();
  const file = await open(path, 'r');
  try {
    const buffer = Buffer.alloc(65537);
    let size = 0;
    while (size < buffer.length) {
      signal.throwIfAborted();
      const result = await file.read(buffer, size, buffer.length - size, null);
      if (!result.bytesRead) return buffer.subarray(0, size).toString('utf8');
      size += result.bytesRead;
    }
    throw new Error('Process metric field limit exceeded.');
  } finally { await file.close(); }
}

/** /proc comm can contain spaces, newlines and closing parentheses. */
export function parseLinuxStat(raw: string): { pid: number; parentPid: number; ticks: string; dead: boolean } {
  const start = raw.indexOf('('), end = raw.lastIndexOf(')');
  const pid = Number(raw.slice(0, start).trim());
  const f = raw.slice(end + 2).trim().split(/\s+/);
  if (start < 1 || end < start || !Number.isSafeInteger(pid) || pid <= 0 ||
    !/^\d+$/.test(f[1] ?? '') || !/^\d+$/.test(f[19] ?? '') || !Number.isSafeInteger(Number(f[1]))) throw new Error('Invalid proc stat.');
  return { pid, parentPid: Number(f[1]), ticks: f[19]!, dead: ['Z', 'X', 'x'].includes(f[0]!) };
}
export function parseLinuxMemory(status: string): number | null {
  const match = /^VmRSS:\s+(\d+)\s+kB\s*$/m.exec(status);
  return match ? bytes(Number(match[1]) * 1024) : null;
}
export function linuxElapsed(ticks: string, uptime: number, hz: number): number | null {
  if (!/^\d+$/.test(ticks) || !Number.isSafeInteger(Number(ticks)) || !nonnegative(uptime) || !Number.isSafeInteger(hz) || hz <= 0) return null;
  const elapsed = uptime - Number(ticks) / hz;
  return nonnegative(elapsed) ? elapsed : null;
}
async function linuxMetrics(snapshot: TrackingSnapshot, signal: AbortSignal): Promise<ProcessMetric[]> {
  const [bootRaw, uptimeRaw, hzRaw] = await Promise.all([
    read('/proc/sys/kernel/random/boot_id', signal), read('/proc/uptime', signal), command('/usr/bin/getconf', ['CLK_TCK'], signal).catch(() => ''),
  ]);
  const boot = bootRaw.trim(), uptime = Number(uptimeRaw.trim().split(/\s+/)[0]), hz = Number(hzRaw.trim());
  if (!/^[a-f\d-]{36}$/i.test(boot)) throw new Error('Boot identity unavailable.');
  const rows: ProcessMetric[] = [];
  let index = 0;
  await Promise.all(Array.from({ length: Math.min(16, snapshot.processes.length) }, async () => {
    while (index < snapshot.processes.length) {
      signal.throwIfAborted();
      const p = snapshot.processes[index++]!;
      if (!p.birth || !p.accessible) continue;
      try {
        const path = '/proc/' + p.pid;
        const before = parseLinuxStat(await read(path + '/stat', signal));
        const memory = parseLinuxMemory(await read(path + '/status', signal));
        const after = parseLinuxStat(await read(path + '/stat', signal));
        if (before.pid !== p.pid || after.pid !== p.pid || before.dead || after.dead || before.ticks !== after.ticks ||
          before.parentPid !== after.parentPid || boot + ':' + before.ticks !== p.birth || before.parentPid !== p.parentPid) continue;
        rows.push({ pid: p.pid, parentPid: before.parentPid, birth: p.birth, memoryBytes: memory, elapsedSeconds: linuxElapsed(before.ticks, uptime, hz) });
      } catch { /* Vanished/reused/inaccessible processes have no metric row. */ }
    }
  }));
  return rows;
}

// Fixed scripts only. Distro names are execFile arguments, never shell source.
const WINDOWS_METRICS = String.raw`
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$now = [DateTime]::UtcNow
$first = @(Get-CimInstance Win32_Process -OperationTimeoutSec 5)
if ($first.Count -gt 20000) { throw 'Process limit' }
$rows = @($first | Where-Object { $_.ProcessId -gt 0 -and $_.CreationDate } | ForEach-Object {
  $born = $_.CreationDate.ToUniversalTime()
  [pscustomobject]@{pid=[int]$_.ProcessId; parentPid=[int]$_.ParentProcessId; birth=$born.ToFileTimeUtc().ToString([Globalization.CultureInfo]::InvariantCulture); memoryBytes=$(if ($null -eq $_.WorkingSetSize) { $null } else { [double]$_.WorkingSetSize }); elapsedSeconds=($now - $born).TotalSeconds}
})
$after = @{}
Get-CimInstance Win32_Process -OperationTimeoutSec 5 | ForEach-Object { $after[[int]$_.ProcessId] = $_ }
$valid = @($rows | Where-Object {
  $p = $after[$_.pid]
  $p -and $p.CreationDate -and $p.CreationDate.ToUniversalTime().ToFileTimeUtc().ToString([Globalization.CultureInfo]::InvariantCulture) -eq $_.birth -and [int]$p.ParentProcessId -eq $_.parentPid
})
ConvertTo-Json -InputObject $valid -Depth 3 -Compress
`;
const WSL_METRICS = String.raw`
import os, json, time
LIMIT = 65536
def read(path):
    with open(path, 'rb') as f: b = f.read(LIMIT + 1)
    if len(b) > LIMIT: raise ValueError('field limit')
    return b
def stat(path):
    b = read(path); f = b[b.rfind(b')')+2:].split()
    return int(f[1]), f[19].decode('ascii'), f[0] in (b'Z', b'X', b'x')
boot = read('/proc/sys/kernel/random/boot_id').decode('ascii').strip()
if len(boot) != 36: raise ValueError('boot identity')
uptime = float(read('/proc/uptime').split()[0]); hz = os.sysconf('SC_CLK_TCK')
pids = [p for p in os.listdir('/proc') if p.isdigit()]
if len(pids) > 20000: raise ValueError('process limit')
rows = []; total = 0; deadline = time.monotonic() + 7
for pid in pids:
    if time.monotonic() > deadline: raise ValueError('deadline')
    try:
        path = '/proc/' + pid; before = stat(path + '/stat'); memory = None
        for line in read(path + '/status').splitlines():
            f = line.split()
            if len(f) == 3 and f[0] == b'VmRSS:' and f[2] == b'kB': memory = int(f[1]) * 1024
        after = stat(path + '/stat')
        if before != after or before[2]: continue
        elapsed = uptime - int(before[1]) / hz
        row = dict(pid=int(pid), parentPid=before[0], birth=boot + ':' + before[1], memoryBytes=memory, elapsedSeconds=elapsed if elapsed >= 0 else None)
        total += len(json.dumps(row))
        if total > 8388608: raise ValueError('output limit')
        rows.append(row)
    except OSError: pass
print(json.dumps(rows))
`;
export function parseMetricRows(raw: string): ProcessMetric[] {
  const data: unknown = JSON.parse(raw.replace(/^\uFEFF/, ''));
  if (!Array.isArray(data) || data.length > MAX_ROWS) throw new Error('Invalid process metrics.');
  const seen = new Set<number>();
  return data.map((p: unknown) => {
    if (!p || typeof p !== 'object') throw new Error('Invalid process metric.');
    const r = p as Record<string, unknown>;
    if (!Number.isSafeInteger(r.pid) || Number(r.pid) <= 0 || !Number.isSafeInteger(r.parentPid) || Number(r.parentPid) < 0 ||
      typeof r.birth !== 'string' || !r.birth || seen.has(Number(r.pid))) throw new Error('Invalid metric identity.');
    seen.add(Number(r.pid));
    return { pid: Number(r.pid), parentPid: Number(r.parentPid), birth: r.birth, memoryBytes: bytes(r.memoryBytes),
      elapsedSeconds: nonnegative(r.elapsedSeconds) ? r.elapsedSeconds : null };
  });
}

/** ps RSS is sampled between two exact-birth snapshots. A seconds-rounded ps
 * start time is never identity evidence. A process cannot regain its old birth. */
export function parseDarwinPs(raw: string, before: TrackingSnapshot, after: TrackingSnapshot, nowSeconds: number): ProcessMetric[] {
  if (before.domain !== after.domain || after.statusOnly) return [];
  const first = new Map(before.processes.map(p => [p.pid, p]));
  const last = new Map(after.processes.map(p => [p.pid, p]));
  const rows: ProcessMetric[] = [], seen = new Set<number>();
  for (const line of raw.split(/\r?\n/).filter(s => s.trim())) {
    const f = /^\s*(\d+)\s+(\d+)\s+(\d+)\s*$/.exec(line);
    if (!f || seen.size >= MAX_ROWS) throw new Error('Invalid ps metrics.');
    const pid = Number(f[1]), parentPid = Number(f[2]), a = first.get(pid), b = last.get(pid);
    if (seen.has(pid)) throw new Error('Duplicate ps PID.');
    seen.add(pid);
    if (!a?.birth || !a.accessible || !b?.accessible || a.birth !== b.birth || a.parentPid !== parentPid || b.parentPid !== parentPid) continue;
    const birth = /^darwin:(\d+):(\d{6})$/.exec(a.birth);
    const elapsed = birth ? nowSeconds - Number(birth[1]) - Number(birth[2]) / 1e6 : NaN;
    rows.push({ pid, parentPid, birth: a.birth, memoryBytes: bytes(Number(f[3]) * 1024), elapsedSeconds: nonnegative(elapsed) ? elapsed : null });
  }
  return rows;
}
function windowsExecutable(name: 'wsl' | 'powershell'): string {
  const system = process.arch === 'ia32' ? 'Sysnative' : 'System32';
  const root = process.env.SystemRoot ?? process.env.WINDIR ?? 'C:\\Windows';
  return name === 'wsl' ? win32.join(root, system, 'wsl.exe') : win32.join(root, system, 'WindowsPowerShell', 'v1.0', 'powershell.exe');
}
export const collectSystemMetrics: MetricsProvider = async (snapshots, roots, signal) => {
  const result: DomainMetrics[] = [];
  let index = 0;
  await Promise.all(Array.from({ length: Math.min(4, snapshots.length) }, async () => {
    while (index < snapshots.length) {
      const s = snapshots[index++]!;
      try {
        let processes: ProcessMetric[];
        if (s.environment === 'wsl') {
          if (process.platform !== 'win32' || !s.distro || s.distro.includes('\0')) throw new Error('WSL unavailable.');
          processes = parseMetricRows(await command(windowsExecutable('wsl'), ['-d', s.distro, '--exec', 'python3', '-c', WSL_METRICS], signal));
        } else if (process.platform === 'linux') processes = await linuxMetrics(s, signal);
        else if (process.platform === 'win32') processes = parseMetricRows(await command(windowsExecutable('powershell'), ['-NoLogo', '-NoProfile', '-NonInteractive', '-Command', WINDOWS_METRICS], signal));
        else if (process.platform === 'darwin') {
          const now = Date.now() / 1000;
          const output = await command('/bin/ps', ['-axo', 'pid=,ppid=,rss='], signal);
          const after = (await createSystemSnapshotProvider()(roots.filter(r => r.environment === 'local'), signal)).filter(a => !a.statusOnly && a.domain === s.domain);
          processes = after.length === 1 ? parseDarwinPs(output, s, after[0]!, now) : [];
        } else throw new Error('Unsupported platform.');
        result.push({ domain: s.domain, processes });
      } catch {
        result.push({ domain: s.domain, processes: [], reason: s.environment === 'wsl'
          ? 'Guest metrics unavailable; WSL and guest Python 3 are required.' : 'Host process metrics unavailable or exceeded their bounds.' });
      }
    }
  }));
  return result;
};
