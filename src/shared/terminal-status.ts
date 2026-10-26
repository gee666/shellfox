import type { SessionDto, TabDto } from './contracts';

export type DotState = { kind: 'error' | 'busy' | 'running' | 'shell' | 'settled'; title: string };
export function tabDot(tab: TabDto, busy = false): DotState {
  if (tab.status === 'error' || tab.error) return { kind: 'error', title: tab.error ? `Error: ${tab.error.message}` : 'Error' };
  if (tab.status === 'running') return { kind: busy ? 'busy' : 'running', title: busy ? 'Agent working' : 'Agent idle' };
  return { kind: 'shell', title: 'Shell' };
}
export function sessionDot(session: SessionDto, busy: Readonly<Record<string, boolean>>): DotState {
  if (session.settledAt || session.status === 'settled') return { kind: 'settled', title: 'Settled' };
  if (session.error) return { kind: 'error', title: `Error: ${session.error.message}` };
  const priority: Record<DotState['kind'], number> = { error: 3, busy: 2, running: 1, shell: 0, settled: -1 };
  return session.tabs.filter(tab => tab.lifecycle !== 'closed').map(tab => tabDot(tab, busy[tab.id]))
    .reduce<DotState>((best, dot) => priority[dot.kind] > priority[best.kind] ? dot : best, { kind: 'shell', title: 'Shell' });
}
