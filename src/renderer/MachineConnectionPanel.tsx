import type { RemotePeer } from '../shared/remote-contracts';
import { Icon } from './components';
import { machineState } from './machine-state';

export function MachineConnectionPanel({ peer, creating, onRetry, onCancel }: {
  peer?: RemotePeer; creating: boolean; onRetry(): void; onCancel(): void;
}) {
  const state = peer ? machineState(peer) : null;
  const waiting = peer?.connection === 'awaiting-approval';
  const busy = state?.tone === 'busy';
  const title = waiting ? 'Waiting for approval' : busy ? 'Connecting to ' + peer?.hostname : peer?.connection === 'rejected' ? 'Request declined' : peer?.online ? 'Machine disconnected' : 'Machine offline';
  return <div className="machine-connection-panel" role="status">
    <div className={`machine-connection-emblem${busy ? ' pending' : ''}`}><Icon name={waiting ? 'shield' : 'server'} /></div>
    <h2>{title}</h2>
    <p>{waiting ? `Approve this machine in Shellfox on ${peer?.hostname}.` : peer?.lastError?.message ?? 'Its terminals are owned by that machine. Reconnect to check their state.'}</p>
    {creating && busy && <small>Your folder prompt will open once the connection is ready.</small>}
    <div>{busy ? <button type="button" onClick={onCancel}>Cancel request</button> : <button type="button" disabled={!peer?.online} onClick={onRetry}>{peer?.connection === 'rejected' ? 'Request access again' : 'Connect'}</button>}</div>
  </div>;
}
