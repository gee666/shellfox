import type { SessionDto } from '../shared/contracts';
export type ControlAction = 'list' | 'stats' | 'processes' | 'inspect' | 'delete' | 'settle' | 'unsettle' | 'tab-open' | 'tab-close' | 'keep-alive';
export interface ControlRequest { action: ControlAction; marker?: string; id?: string; current?: boolean; confirmed?: boolean; }
export type { ProcessStatDto as ProcessStat, TabStatDto as TabStat } from '../shared/process-stats-schemas';
import type { TabStatDto as TabStat } from '../shared/process-stats-schemas';
export interface ControlResponse { sessions?: SessionDto[]; session?: SessionDto; tabs?: TabStat[]; message?: string; }
