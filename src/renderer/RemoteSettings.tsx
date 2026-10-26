import { useEffect, useRef, useState } from 'react';
import { useStore } from 'zustand';
import type { ManagerApi } from '../shared/contracts';
import type { RemoteController } from './remote-controller';
import type { RemotePeer } from '../shared/remote-contracts';
import { Icon } from './components';
import { useSettingsLogin } from './settings-login';

export function RemoteSettings({ controller, local }: { controller: RemoteController; local: ManagerApi }) {
  const { state, supported } = useStore(controller.store);
  const [hostname, setHostname] = useState(state?.hostname ?? 'shellfox');
  const [authKey, setAuthKey] = useState('');
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<{ context: 'access' | 'machines' | 'clients'; message: string } | null>(null);
  const [revoking, setRevoking] = useState<string | null>(null);
  const locked = useRef(false);
  const login = useSettingsLogin(controller, local, state);
  useEffect(() => { if (state) setHostname(state.hostname); }, [state?.hostname]);
  const api = controller.api;
  if (!api || !supported) return null;
  async function run(operation: Parameters<RemoteController['run']>[0], context: 'access' | 'machines' | 'clients' = 'access') {
    if (locked.current) return;
    locked.current = true; setPending(true); setError(null);
    try { const result = await controller.run(operation, { reportError: false }); if (!result.ok) setError({ context, message: result.error.message.split('\n')[0]?.slice(0, 200) ?? 'Request failed.' }); }
    finally { locked.current = false; setPending(false); }
  }
  function connection(peer: RemotePeer) {
    if (!peer.online) return 'Offline';
    return { 'awaiting-approval': 'Waiting for approval', rejected: 'Request declined', connected: 'Connected', connecting: 'Connecting…', reconnecting: 'Reconnecting…', disconnected: 'Disconnected' }[peer.connection];
  }
  function requestControl(peer: RemotePeer) {
    if (peer.connection === 'awaiting-approval') return <button disabled={pending} aria-label={`Cancel request to ${peer.hostname}`} onClick={() => void run(() => api!.disconnect({ peerId: peer.id }), 'machines')}>Cancel request</button>;
    if (peer.connection === 'rejected') return <button disabled={pending || !peer.online} aria-label={`Request access to ${peer.hostname} again`} onClick={() => void run(() => api!.connect({ peerId: peer.id }), 'machines')}>Request again</button>;
    return null;
  }
  function operationError(context: 'access' | 'machines' | 'clients') { return error?.context === context ? <p className="field-error" role="alert">{error.message}</p> : null; }
  const online = state?.status === 'online';
  const status = !state?.enabled ? 'Disabled' : login.busy ? 'Signing in' : online ? 'Connected' : state.status === 'starting' ? 'Starting' : state.status === 'error' ? 'Connection error' : 'Not connected';
  return <div className="remote-settings">
    <section><div className="settings-section-heading"><h3>Remote access</h3><button role="switch" aria-label="Allow remote access" aria-checked={state?.enabled ?? false} className="toggle" disabled={!state || pending || login.busy} onClick={() => state && void run(() => api.configure({ enabled: !state.enabled, hostname: state.hostname }))}><span /></button></div>
      <p className="settings-hint">Let approved clients use sessions on this device.</p>
      {!state ? <p className="settings-hint">Reading connection status…</p> : <>
        <form className="remote-identity" onSubmit={event => { event.preventDefault(); void run(() => api.configure({ enabled: state.enabled, hostname: hostname.trim() })); }}>
          <label htmlFor="remote-hostname">Machine name</label><input id="remote-hostname" aria-label="Remote machine name" value={hostname} maxLength={63} onChange={event => setHostname(event.target.value)} /><button disabled={pending || !hostname.trim() || hostname === state.hostname}>Save</button>
        </form>
        <div className="remote-account"><div className="remote-account-title"><span>Tailscale</span><span className={`connection-badge${online ? ' connected' : ''}`} role="status">{status}</span></div>
          {online ? <p className="settings-hint">This device is connected to your tailnet.</p> : <p className="settings-hint">{!state.enabled ? 'Enable remote access to connect this device.' : login.message || state.error || 'Sign in to find your other machines.'}</p>}
          {!online && state.enabled && <div className="remote-login-actions">
            {login.busy ? <><span className="connection-progress" aria-hidden="true" /><button onClick={login.cancel}>Cancel sign-in</button></> : <button className="settings-primary" onClick={() => login.phase === 'error' ? login.retry() : void login.start()}>{login.phase === 'error' || state.status === 'error' ? 'Retry sign-in' : 'Sign in to Tailscale'}</button>}
            {(login.url || state.status === 'login-required' && state.loginUrl) && <button onClick={login.continueLogin}>Continue in browser</button>}
          </div>}
          {!online && login.phase === 'error' && <p className="field-error" role="alert">{login.message}</p>}
          {!online && login.url && login.phase === 'error' && <input className="remote-login-url" aria-label="Tailscale login address" readOnly value={login.url} onFocus={event => event.currentTarget.select()} />}
          {state.enabled && !online && <details className="remote-advanced"><summary>Advanced sign-in</summary>
            <form className="remote-key-form" onSubmit={event => { event.preventDefault(); const key = authKey.trim(); setAuthKey(''); if (key) void login.start(key); }}>
              <label htmlFor="tailscale-auth-key">Auth key</label><input id="tailscale-auth-key" type="password" aria-label="Tailscale auth key" autoComplete="off" placeholder="tskey-auth-…" value={authKey} onChange={event => setAuthKey(event.target.value)} /><button disabled={login.busy || !authKey.trim()}>Use key</button>
            </form><small>Used once. Not saved by Shellfox.</small>
          </details>}
        </div>
      </>}
      {operationError('access')}
    </section>
    {state && <>
      <section><div className="settings-section-heading"><h3>Machines</h3><small>{state.peers.filter(peer => peer.remembered).length} saved</small></div>
        <p className="settings-hint">Connect to Shellfox on another device. Sessions run there, not here.</p>
        <div className="remote-peer-list">{state.peers.filter(peer => peer.remembered).map(peer => <div className="remote-peer-row" key={peer.id}><span>{peer.hostname}<small>{connection(peer)}{peer.lastError ? ` · ${peer.lastError.message}` : ''}</small></span>{requestControl(peer)}<button className="icon-button" disabled={pending} aria-label={`Forget ${peer.hostname}`} title="Forget machine" onClick={() => void run(() => api.forget({ peerId: peer.id }), 'machines')}><Icon name="close" /></button></div>)}{!state.peers.some(peer => peer.remembered) && <p className="settings-empty">No saved machines</p>}</div>
        <h4>Available on your tailnet</h4><div className="remote-peer-list">{state.peers.filter(peer => !peer.remembered).map(peer => <div className="remote-peer-row" key={peer.id}><span>{peer.hostname}<small>{peer.connection === 'awaiting-approval' || peer.connection === 'rejected' ? connection(peer) : peer.addresses.join(', ')}</small></span>{requestControl(peer) ?? <button disabled={pending || !peer.online || peer.connection === 'connecting'} aria-label={`Connect ${peer.hostname}`} onClick={() => void run(() => api.connect({ peerId: peer.id }), 'machines')}>{peer.connection === 'connecting' ? 'Connecting…' : 'Connect'}</button>}</div>)}{!state.peers.some(peer => !peer.remembered) && <p className="settings-empty">No other machines found</p>}</div>
        {operationError('machines')}
      </section>
      <section id="client-access-settings" tabIndex={-1} aria-label="Client access"><div className="settings-section-heading"><h3>Client access</h3>{state.pending.length > 0 && <span className="connection-badge">{state.pending.length} pending</span>}</div>
        <p className="settings-hint">Approve only devices you recognize. Revoking access leaves sessions running.</p>
        {state.pending.length > 0 && <><h4>Waiting for approval</h4>{state.pending.map(peer => <div className="remote-peer-row" key={peer.id}><span>{peer.hostname}<small className="remote-device-id" id={`pending-device-${peer.id}`}>Device ID: {peer.id}</small>{peer.addresses.length > 0 && <small>{peer.addresses.join(', ')}</small>}</span><button disabled={pending} aria-label={`Approve ${peer.hostname}`} aria-describedby={`pending-device-${peer.id}`} title={`Device ID: ${peer.id}`} onClick={() => void run(() => api.approve({ peerId: peer.id }), 'clients')}>Approve</button><button className="icon-button" title="Reject client" aria-label={`Reject ${peer.hostname}`} aria-describedby={`pending-device-${peer.id}`} disabled={pending} onClick={() => void run(() => api.reject({ peerId: peer.id }), 'clients')}><Icon name="close" /></button></div>)}</>}
        {state.approvals.map(peer => <div className="remote-peer-row" key={peer.peerId}><span>{peer.hostname}<small className="remote-device-id" id={`approved-device-${peer.peerId}`}>Device ID: {peer.peerId}</small></span>{revoking === peer.peerId ? <div className="remote-revoke-confirm" role="group" aria-label={`Revoke access for ${peer.hostname}`}><span>Revoke access?</span><button disabled={pending} aria-label={`Confirm revoke ${peer.hostname}`} aria-describedby={`approved-device-${peer.peerId}`} title={`Device ID: ${peer.peerId}`} onClick={() => void run(() => api.revoke({ peerId: peer.peerId }), 'clients').then(() => setRevoking(null))}>Revoke</button><button className="text-button" onClick={() => setRevoking(null)}>Cancel</button></div> : <button className="text-button" aria-label={`Revoke ${peer.hostname}`} aria-describedby={`approved-device-${peer.peerId}`} title={`Device ID: ${peer.peerId}`} disabled={pending} onClick={() => setRevoking(peer.peerId)}>Revoke</button>}</div>)}
        {!state.pending.length && !state.approvals.length && <p className="settings-empty">No clients have access</p>}
        {operationError('clients')}
      </section>
    </>}
  </div>;
}
