import { useRef, useState } from 'react';
import type { RemotePeer } from '../shared/remote-contracts';
import { ContextMenu, Icon } from './components';
import { machineState } from './machine-state';

export function NewSessionButton({ peers, selectedMachine, localAvailable, busy, onChoose }: {
  peers: RemotePeer[]; selectedMachine: string; localAvailable: boolean; busy: boolean; onChoose(machineId: string): void;
}) {
  const anchor = useRef<HTMLButtonElement>(null);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const machines = peers.filter(peer => peer.remembered);
  function open() {
    if (!machines.length) { onChoose('local'); return; }
    if (menu) { setMenu(null); return; }
    const box = anchor.current?.getBoundingClientRect();
    if (box) setMenu({ x: box.left, y: box.bottom + 5 });
  }
  function choose(id: string) { setMenu(null); onChoose(id); }
  return <>
    <button ref={anchor} type="button" className={`icon-button${menu ? ' active' : ''}`} aria-label="New session" aria-haspopup={machines.length ? 'menu' : undefined}
      aria-expanded={machines.length ? !!menu : undefined} data-tip={busy ? 'Preparing session…' : 'New session'} disabled={busy || (!localAvailable && !machines.length)} onClick={open}><Icon name="plus" /></button>
    {menu && <ContextMenu x={menu.x} y={menu.y} onClose={() => setMenu(null)} initialFocus="button[data-selected='true']:not(:disabled)"><div className="session-target-menu">
      <div className="session-target-caption">New session on</div>
      <div className="session-target-items">
        <button type="button" role="menuitem" className="session-target-item" data-selected={selectedMachine === 'local'} disabled={!localAvailable} onClick={() => choose('local')}>
          <Icon name="monitor" /><span>Local</span>{selectedMachine === 'local' && <Icon name="check" />}
        </button>
        {machines.map(peer => {
          const status = machineState(peer), connected = peer.connection === 'connected';
          return <button type="button" role="menuitem" className="session-target-item" data-selected={selectedMachine === peer.id} key={peer.id} disabled={!peer.online && !connected} onClick={() => choose(peer.id)}>
            <Icon name="server" /><span>{peer.hostname}<small>{connected ? 'Connected' : peer.online ? 'Connect to continue' : 'Offline'}</small></span>
            {selectedMachine === peer.id ? <Icon name="check" /> : <span className={`machine-connection-dot ${status.tone}`} />}
          </button>;
        })}
      </div>
    </div></ContextMenu>}
  </>;
}
