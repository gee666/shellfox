import { randomUUID } from 'node:crypto';
import { vi } from 'vitest';
import type { SettingsDto, TerminalProfilesDto } from '../../shared/contracts';
import type { RepositoryPort, SessionRecord, TabRecord, OperationRecord } from '../models';
import { defaultSettings } from '../defaults';
import type { PtyFactory, PtyProcess } from './backend';
export const profiles: TerminalProfilesDto = { profiles: [{ id: 'login-shell', label: 'Bash', environment: 'local', executable: '/bin/bash', args: ['-l', '-i'], distro: null, available: true }, { id: 'wsl:Ubuntu', label: 'Ubuntu', environment: 'wsl', executable: 'C:\\Windows\\System32\\wsl.exe', args: ['--distribution', 'Ubuntu', '--exec', '/bin/bash', '-l', '-i'], distro: 'Ubuntu', available: true }], defaultProfileId: 'login-shell', lifetime: 'app-owned', shellSurvival: false };
export class FakePty implements PtyProcess {
  pid = 123; queuedInputBytes = vi.fn((): number | null => 0); write = vi.fn(); resize = vi.fn(); kill = vi.fn(() => this.exit(0));
  private data = new Set<(data: string) => void>(); private exits = new Set<(e: { exitCode: number; signal?: number }) => void>();
  onData(fn: (data: string) => void) { this.data.add(fn); return { dispose: () => { this.data.delete(fn); } }; }
  onExit(fn: (e: { exitCode: number; signal?: number }) => void) { this.exits.add(fn); return { dispose: () => { this.exits.delete(fn); } }; }
  output(data: string) { for (const fn of this.data) fn(data); }
  exit(exitCode: number, signal?: number) { for (const fn of [...this.exits]) fn({ exitCode, signal }); }
}
export function factoryFixture() {
  const processes: FakePty[] = [];
  const factory: PtyFactory = vi.fn(() => { const p = new FakePty(); p.pid += processes.length; processes.push(p); return p; });
  return { factory, processes, options: { platform: 'win32' as NodeJS.Platform, factory, discover: async () => profiles, validateCwd: async (_profile: unknown, cwd: string) => cwd, terminateGuest: async (root: { pid: number }) => { processes.find(p => p.pid === root.pid)?.kill(); }, terminateUnix: async (root: { pid: number }) => { processes.find(p => p.pid === root.pid)?.kill(); } } };
}
export function launchInput() { return { tabId: randomUUID(), sessionId: randomUUID(), generation: randomUUID(), profileId: 'login-shell', cwd: '/work' }; }
export class MemoryRepository implements RepositoryPort {
  sessionMap = new Map<string, SessionRecord>(); tabMap = new Map<string, TabRecord>(); operationMap = new Map<string, OperationRecord>();
  config = structuredClone(defaultSettings); preference = false;
  sessions() { return [...this.sessionMap.values()].map(s => structuredClone(s)); }
  session(id: string) { const s = this.sessionMap.get(id); return s && structuredClone(s); }
  tabs(id?: string) { return [...this.tabMap.values()].filter(t => !id || t.sessionId === id).sort((a, b) => a.ordinal - b.ordinal).map(t => structuredClone(t)); }
  tab(id: string) { const t = this.tabMap.get(id); return t && structuredClone(t); }
  saveSession(s: SessionRecord) { this.sessionMap.set(s.id, structuredClone(s)); }
  saveTab(t: TabRecord) { this.tabMap.set(t.id, structuredClone(t)); }
  deleteSession(id: string) {
    if (!this.sessionMap.get(id)?.settledAt) return false;
    for (const tab of this.tabs(id)) this.tabMap.delete(tab.id);
    for (const op of [...this.operationMap.values()]) if (op.sessionId === id) this.operationMap.delete(op.id);
    return this.sessionMap.delete(id);
  }
  saveOperation(o: OperationRecord) { this.operationMap.set(o.id, structuredClone(o)); }
  operation(id: string) { const o = this.operationMap.get(id); return o && structuredClone(o); }
  operationsForTab(id: string) { return [...this.operationMap.values()].filter(o => o.tabId === id).map(o => structuredClone(o)); }
  sessionByRequest(id: string) { const op = [...this.operationMap.values()].find(o => o.requestId === id); return op && this.session(op.sessionId); }
  transaction<T>(fn: () => T) { const saved = structuredClone([this.sessionMap, this.tabMap, this.operationMap, this.config]); try { return fn(); } catch (e) { [this.sessionMap, this.tabMap, this.operationMap, this.config] = saved as [Map<string, SessionRecord>, Map<string, TabRecord>, Map<string, OperationRecord>, SettingsDto]; throw e; } }
  settings() { return structuredClone(this.config); }
  saveSettings(settings: SettingsDto) { this.config = structuredClone(settings); }
  settled(search: string) { return this.sessions().filter(s => s.settledAt && (s.title.toLowerCase().includes(search.toLowerCase()) || s.cwd.toLowerCase().includes(search.toLowerCase()))).sort((a, b) => b.settledAt!.localeCompare(a.settledAt!) || a.id.localeCompare(b.id)); }
  explorerPreference() { return this.preference; }
  saveExplorerPreference(value: boolean) { this.preference = value; }
  close() {}
}
