import { useEffect, useRef, useState } from 'react';
import type { SessionDto } from '../shared/contracts';
import type { ManagerClient } from './store';
import { request } from './api';
import { Modal, useNotify } from './components';
import { formatShellfoxEnv, parseShellfoxEnv } from './shellfox-env';

export function ShellfoxEnvModal({ client, session, onClose }: {
  client: ManagerClient; session: SessionDto; onClose: () => void;
}) {
  const [text, setText] = useState(() => formatShellfoxEnv(session.env));
  const [pending, setPending] = useState(false);
  const locked = useRef(false);
  const mounted = useRef(true);
  const notify = useNotify();
  const platform = client.store.getState().snapshot?.probe.platform ?? 'win32';
  const { env, issues } = parseShellfoxEnv(text, platform === 'win32');
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  async function save() {
    if (locked.current || issues.length) return;
    locked.current = true; setPending(true);
    try {
      const result = await request(() => client.api.setSessionEnv({ sessionId: session.id, env }));
      if (!result.ok) { notify(result.error); return; }
      // Editing a non-selected row must not switch the active session.
      client.updateHistory([result.value]); client.refresh();
      if (mounted.current) onClose();
    } finally {
      locked.current = false;
      if (mounted.current) setPending(false);
    }
  }
  return <Modal title={`Environment · ${session.title}`} onClose={onClose} className="shellfox-env-modal" initialFocus="textarea">
    <form className="shellfox-env-body" onSubmit={event => { event.preventDefault(); void save(); }}
      onKeyDown={event => { if (event.ctrlKey && event.key === 'Enter') { event.preventDefault(); void save(); } }}>
      <textarea aria-label="Environment variables" aria-invalid={issues.length > 0} aria-describedby="shellfox-env-note shellfox-env-errors"
        value={text} readOnly={pending} onChange={event => setText(event.target.value)} spellCheck={false} wrap="off"
        placeholder={'# One per line\nAPI_KEY=sk-...\nNODE_ENV=development'} />
      <div className="shellfox-env-feedback">
        <p id="shellfox-env-note">Applies to new terminals in this session.</p>
        <div id="shellfox-env-errors" className="shellfox-env-errors" aria-live="polite">{issues.map(issue => <small className="field-error" key={issue.line}>Line {issue.line}: {issue.message}</small>)}</div>
      </div>
      <footer><button type="button" onClick={onClose}>Cancel</button><button type="submit" disabled={pending || issues.length > 0} title="Ctrl+Enter">Save</button></footer>
    </form>
  </Modal>;
}
