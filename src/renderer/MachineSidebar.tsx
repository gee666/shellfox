import { useEffect, useState, type ReactNode, type RefObject } from 'react';
import { useStore } from 'zustand';
import type { RemotePeer } from '../shared/remote-contracts';
import type { MachineRuntime, RemoteController } from './remote-controller';
import { SessionSidebar } from './SessionSidebar';
import { Icon, useNotify } from './components';
import { useVisibleSessions } from './visible-sessions';
import { machineState } from './machine-state';

export function MachineSidebar({ controller, selectedMachine, onSelect, pageSize, scrollContainer, root }: {
  controller: RemoteController; selectedMachine: string; onSelect: (id: string) => void; pageSize: number; scrollContainer: RefObject<HTMLDivElement | null>; root: RefObject<HTMLDivElement | null>;
}) {
  const { state, runtimes } = useStore(controller.store);
  return <>{state?.peers.filter(peer => peer.remembered).map(peer => <MachineSection key={peer.id} peer={peer} runtime={runtimes[peer.id]} controller={controller}
    selected={selectedMachine === peer.id} onSelect={() => onSelect(peer.id)} pageSize={pageSize} scrollContainer={scrollContainer} root={root} />)}</>;
}

function MachineSection({ peer, runtime, controller, selected, onSelect, pageSize, scrollContainer, root }: {
  peer: RemotePeer; runtime?: MachineRuntime; controller: RemoteController; selected: boolean; onSelect: () => void; pageSize: number; scrollContainer: RefObject<HTMLDivElement | null>; root: RefObject<HTMLDivElement | null>;
}) {
  const [expanded, setExpanded] = useState(true);
  const [pending, setPending] = useState(false);
  const state = machineState(peer);
  const active = state.active;
  async function toggle() {
    const api = controller.api;
    if (!api || pending) return;
    setPending(true);
    try { await controller.run(() => active ? api.disconnect({ peerId: peer.id }) : api.connect({ peerId: peer.id })); }
    finally { setPending(false); }
  }
  const controls = <div className="machine-controls"><span className={`machine-connection-dot ${state.tone}`} role="img" aria-label={state.label} title={state.detail} /><button type="button" className={`icon-button machine-connection-button${state.tone === 'busy' ? ' connecting' : ''}`} aria-label={`${active ? 'Disconnect' : 'Connect'} ${peer.hostname}`} title={active ? `${state.detail} Disconnect` : peer.connection === 'rejected' ? `${state.detail} Request access again` : `Connect to ${peer.hostname}`} aria-busy={pending || state.tone === 'busy'} disabled={pending || (!peer.online && !active)} onClick={() => void toggle()}><Icon name={active ? 'disconnect' : 'connect'} /></button></div>;
  return <section data-machine-id={peer.id} className={`machine-section${selected ? ' machine-selected' : ''}`} aria-label={peer.hostname}>
    {runtime ? <ConnectedMachine runtime={runtime} selected={selected} label={peer.hostname} onSelect={onSelect} pageSize={pageSize} controls={controls} scrollContainer={scrollContainer} root={root} machineId={peer.id} /> : <>
      <div className="machine-section-heading"><button className="session-section-toggle" aria-expanded={expanded} onClick={() => { setExpanded(value => !value); onSelect(); }}><span className={`section-chevron${expanded ? ' expanded' : ''}`}><Icon name="chevron" /></span><span className="section-label">{peer.hostname}</span></button>{controls}</div>
      {expanded && <div role="region" aria-label={`${peer.hostname} sessions`} />}
    </>}
  </section>;
}

function ConnectedMachine({ runtime, selected, label, onSelect, pageSize, controls, scrollContainer, root, machineId }: {
  runtime: MachineRuntime; selected: boolean; label: string; onSelect: () => void; pageSize: number; controls: ReactNode; scrollContainer: RefObject<HTMLDivElement | null>; root: RefObject<HTMLDivElement | null>; machineId: string;
}) {
  useVisibleSessions(runtime.client.api, root, machineId);
  const state = useStore(runtime.client.store);
  const notify = useNotify();
  useEffect(() => { if (state.error && !state.snapshot) notify(state.error); }, [state.error, state.snapshot]);
  return <SessionSidebar client={runtime.client} sessions={state.snapshot?.sessions ?? []} selectedId={selected ? state.selectedId : null}
    busy={state.busyTabIds} pageSize={pageSize} label={label} remote actions={controls} scrollContainer={scrollContainer} onSelect={onSelect} onSectionSelect={onSelect} />;
}
