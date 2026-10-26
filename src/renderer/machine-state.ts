import type { RemotePeer } from '../shared/remote-contracts';

export function machineState(peer: RemotePeer) {
  switch (peer.connection) {
    case 'connected': return { label: 'Connected', detail: 'Connected to ' + peer.hostname, tone: 'connected', active: true };
    case 'awaiting-approval': return { label: 'Waiting for approval', detail: 'Approve this request in Shellfox on ' + peer.hostname + '.', tone: 'busy', active: true };
    case 'connecting': return { label: 'Connecting', detail: 'Connecting to ' + peer.hostname + '…', tone: 'busy', active: true };
    case 'reconnecting': return { label: 'Reconnecting', detail: peer.lastError?.message ?? 'Reconnecting to ' + peer.hostname + '…', tone: 'busy', active: true };
    case 'rejected': return { label: 'Access denied', detail: peer.lastError?.message ?? 'The host declined the request. You can request access again.', tone: 'denied', active: false };
    default: return { label: peer.online ? 'Disconnected' : 'Offline', detail: peer.online ? 'Ready to connect' : 'This machine is offline.', tone: 'offline', active: false };
  }
}
