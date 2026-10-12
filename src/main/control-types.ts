import type { SessionDto } from '../shared/contracts';
export type ControlAction = 'list' | 'stats' | 'processes' | 'inspect' | 'delete' | 'archive' | 'tab-open' | 'tab-close' | 'keep-alive';
export interface ControlRequest { action: ControlAction; marker?: string; id?: string; current?: boolean; confirmed?: boolean; }
export interface ProcessStat { pid: number; parentPid: number; name: string; memoryBytes: number | null; elapsedSeconds: number | null; domain: string; }
export interface TabStat { tabId: string; processes: ProcessStat[]; memoryBytes: number | null; elapsedSeconds: number | null; reason?: string; }
export interface ControlResponse { sessions?: SessionDto[]; session?: SessionDto; tabs?: TabStat[]; message?: string; }
