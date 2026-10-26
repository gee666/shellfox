import type { RemotePeer } from '../../src/shared/remote-contracts';
import type { AppError } from '../../src/shared/contracts';

type Connection = RemotePeer['connection'];
export type ConnectionLabel = 'connected' | 'connecting...' | 'reconnecting...' | 'disconnected' | 'awaiting approval' | 'access denied';
export function connectionLabel(state: Connection): ConnectionLabel {
  switch (state) {
    case 'connected': return 'connected';
    case 'connecting': return 'connecting...';
    case 'reconnecting': return 'reconnecting...';
    case 'awaiting-approval': return 'awaiting approval';
    case 'rejected': return 'access denied';
    case 'disconnected': return 'disconnected';
  }
}
export function connectionSummary(states: readonly Connection[], starting = false): string {
  if (!states.length) return starting ? 'connecting...' : 'disconnected';
  const labels = states.map(connectionLabel);
  if (labels.every(label => label === labels[0])) return labels[0];
  const counts = new Map<string, number>();
  for (const label of labels) counts.set(label, (counts.get(label) ?? 0) + 1);
  return [...counts].map(([label, count]) => `${count} ${label}`).join(' · ');
}
export function isConnectionFailure(error: AppError): boolean {
  return ['NATIVE_UNAVAILABLE', 'AUTH_FAILED', 'APPROVAL_REQUIRED', 'ACCESS_DENIED'].includes(error.code);
}
