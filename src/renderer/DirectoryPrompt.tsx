import { useEffect, useRef, useState } from 'react';
import type { ManagerApi } from '../shared/contracts';
import { request } from './api';
import { Icon, Modal } from './components';

export function DirectoryPrompt({ api, onCreate, onClose }: { api: ManagerApi; onCreate: (cwd: string) => Promise<boolean>; onClose: () => void }) {
  const [home, setHome] = useState('~');
  const [path, setPath] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const alive = useRef(true);
  const locked = useRef(false);
  const revision = useRef(0);
  const completion = useRef<{ matches: string[]; index: number } | null>(null);
  useEffect(() => {
    alive.current = true;
    let active = true;
    const ticket = revision.current;
    void request(() => api.getHomeDirectory()).then(result => {
      if (!active) return;
      if (result.ok) setHome(result.value.cwd);
      else if (revision.current === ticket) setError(result.error.message);
    });
    return () => { active = false; alive.current = false; revision.current++; };
  }, [api]);
  async function open(browse: boolean) {
    if (locked.current) return;
    locked.current = true; setBusy(true); setError(''); revision.current++;
    try {
      const result = await request(() => browse ? api.chooseDirectory() : api.resolveDirectory({ path }), browse ? 0 : undefined);
      if (!alive.current) return;
      if (!result.ok) { setError(result.error.message); return; }
      if (result.value && await onCreate(result.value.cwd) && alive.current) onClose();
    } catch {
      if (alive.current) setError('Could not create the session. Check the session state before trying again.');
    } finally {
      locked.current = false;
      if (alive.current) { setBusy(false); input.current?.focus(); }
    }
  }
  async function complete() {
    if (locked.current) return;
    const ticket = ++revision.current;
    if (!completion.current) {
      const result = await request(() => api.completeDirectory({ path }));
      if (!alive.current || revision.current !== ticket) return;
      if (!result.ok) { setError(result.error.message); return; }
      if (!result.value.matches.length) { setError('No matching folders'); return; }
      completion.current = { matches: result.value.matches, index: -1 };
    }
    const cycle = completion.current;
    cycle.index = (cycle.index + 1) % cycle.matches.length;
    setPath(cycle.matches[cycle.index]!); setError('');
    // A unique match is accepted. The next Tab should list its children, not repeat it.
    if (cycle.matches.length === 1) completion.current = null;
  }
  return <Modal title="New session" className="directory-modal" initialFocus="input" onClose={() => { if (!locked.current) onClose(); }}>
    <form className="directory-form" onSubmit={event => { event.preventDefault(); void open(false); }}>
      <div className="directory-line">
        <span className="directory-cwd" title={home} aria-label={`Working directory: ${home}`}>~</span><span className="directory-chevron" aria-hidden="true">›</span>
        <input ref={input} aria-label="Folder path" aria-describedby="directory-hint" placeholder="folder path" value={path} readOnly={busy} autoComplete="off" spellCheck={false}
          onChange={event => { revision.current++; completion.current = null; setPath(event.target.value); setError(''); }}
          onKeyDownCapture={event => { if (event.key === 'Tab') { event.preventDefault(); event.stopPropagation(); if (event.shiftKey) input.current?.parentElement?.querySelector('button')?.focus(); else void complete(); } }} />
        <button type="button" className="icon-button" aria-label="Browse folders" title="Use Explorer" disabled={busy} onClick={() => void open(true)}><Icon name="folder" /></button>
      </div>
      <div id="directory-hint" className="directory-hint">Tab complete · Enter open · Shift+Tab browse</div>
      {error && <div className="field-error" role="alert">{error}</div>}
    </form>
  </Modal>;
}
