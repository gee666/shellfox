import { useEffect, useRef, useState } from 'react';
import type { CloseConfirmation, CloseConfirmationApi, ManagerApi } from '../shared/contracts';
import { request } from './api';
import { Modal } from './components';

export function CloseConfirmationModal({ api }: { api: ManagerApi }) {
  const [confirmation, setConfirmation] = useState<CloseConfirmation | null>(null);
  useEffect(() => {
    if (!api.getCloseConfirmation || !api.subscribeCloseConfirmation || !api.respondCloseConfirmation) return;
    let active = true;
    let changed = false;
    const stop = api.subscribeCloseConfirmation(value => { changed = true; if (active) setConfirmation(value); });
    // Subscribe before reading so an older response cannot erase a new prompt.
    void request(() => api.getCloseConfirmation!()).then(result => {
      if (active && !changed && result.ok) setConfirmation(result.value);
    });
    return () => { active = false; stop(); };
  }, [api]);
  if (!confirmation || !api.respondCloseConfirmation) return null;
  return <ClosePrompt key={confirmation.id} confirmation={confirmation} respond={api.respondCloseConfirmation} onAnswered={() => setConfirmation(current => current?.id === confirmation.id ? null : current)} />;
}

function ClosePrompt({ confirmation, respond, onAnswered }: {
  confirmation: CloseConfirmation; respond: CloseConfirmationApi['respondCloseConfirmation']; onAnswered(): void;
}) {
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const locked = useRef(false);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  const update = confirmation.kind === 'update';
  async function answer(approved: boolean) {
    if (locked.current) return;
    locked.current = true; setPending(true); setError(null);
    const result = await request(() => respond({ id: confirmation.id, approved }));
    if (!mounted.current) return;
    locked.current = false; setPending(false);
    if (result.ok) onAnswered();
    else setError(result.error.message);
  }
  const count = confirmation.terminalCount;
  return <Modal title={update ? 'Install update?' : 'Quit Shellfox?'} className="close-app-modal" initialFocus="[data-cancel-close]" onClose={() => void answer(false)}>
    <div className="close-app-body">
      <p>{count ? `Close ${count} shell${count === 1 ? '' : 's'}${update ? ' and install the update' : ' and quit'}?` : 'Install the update and restart Shellfox?'}</p>
      {count > 0 && <p className="settings-hint">Running commands will stop. Shells restart fresh next time. External terminals stay open.</p>}
      {error && <small className="field-error" role="alert">{error}</small>}
      <footer><button data-cancel-close disabled={pending} onClick={() => void answer(false)}>Cancel</button><button disabled={pending} onClick={() => void answer(true)}>{update ? 'Close shells and install' : 'Close shells and quit'}</button></footer>
    </div>
  </Modal>;
}
