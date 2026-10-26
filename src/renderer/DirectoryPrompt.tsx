import { useEffect, useRef, useState } from 'react';
import type { ManagerApi } from '../shared/contracts';
import { request } from './api';
import { Icon, Modal } from './components';

export function DirectoryPrompt({ api, onCreate, onClose, remote = false, machineName }: {
  api: ManagerApi; onCreate(cwd: string): Promise<boolean>; onClose(): void; remote?: boolean; machineName?: string;
}) {
  const [home, setHome] = useState('~');
  const [path, setPath] = useState('');
  const [error, setError] = useState('');
  const [missing, setMissing] = useState<string | null>(null);
  const [busy, setBusy] = useState<'checking' | 'creating' | null>(null);
  const input = useRef<HTMLInputElement>(null), editButton = useRef<HTMLButtonElement>(null);
  const alive = useRef(true), locked = useRef(false), revision = useRef(0);
  const completion = useRef<{ matches: string[]; index: number } | null>(null);
  useEffect(() => {
    alive.current = true; let active = true;
    const ticket = revision.current;
    void request(() => api.getHomeDirectory()).then(result => {
      if (!active) return;
      if (result.ok) setHome(result.value.cwd);
      else if (revision.current === ticket) setError(result.error.message);
    });
    return () => { active = false; alive.current = false; revision.current++; };
  }, [api]);
  useEffect(() => { if (missing !== null) editButton.current?.focus(); }, [missing]);
  function editPath() { setMissing(null); setError(''); input.current?.focus(); }
  function close() { if (locked.current) return; if (missing !== null) editPath(); else onClose(); }
  async function open(browse: boolean, createMissing = false) {
    if (locked.current || browse && remote) return;
    if (missing !== null && !createMissing && !browse) return;
    const createDirectory = api.createDirectory;
    if (createMissing && (!createDirectory || missing === null)) return;
    const candidate = createMissing ? missing : path;
    if (candidate === null) return;
    let focusPath = true;
    locked.current = true; setBusy(createMissing ? 'creating' : 'checking'); setError(''); revision.current++;
    try {
      const result = await request(() => createMissing && createDirectory ? createDirectory({ path: candidate }) : browse ? api.chooseDirectory() : api.resolveDirectory({ path: candidate }), browse ? 0 : undefined);
      if (!alive.current) return;
      if (!result.ok) {
        if (!browse && !createMissing && result.error.code === 'NOT_FOUND' && createDirectory) { focusPath = false; setMissing(candidate); return; }
        setMissing(null); setError(result.error.message); return;
      }
      setMissing(null);
      if (result.value && await onCreate(result.value.cwd) && alive.current) onClose();
    } catch {
      if (alive.current) { setMissing(null); setError('The session was not confirmed. Check its state before trying again.'); }
    } finally {
      locked.current = false;
      if (alive.current) { setBusy(null); if (focusPath) input.current?.focus(); }
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
    if (cycle.matches.length === 1) completion.current = null;
  }
  return <Modal title="New session" className="directory-modal" initialFocus="input" onClose={close}>
    <form className="directory-form" onSubmit={event => { event.preventDefault(); void open(false); }}>
      {machineName && <div className="directory-machine"><Icon name={remote ? 'server' : 'monitor'} /><span>{machineName}</span>{remote && <small>Remote folder</small>}</div>}
      <div className="directory-line">
        <span className="directory-cwd" title={home} aria-label={`Working directory: ${home}`}>~</span><span className="directory-chevron" aria-hidden="true">›</span>
        <input ref={input} aria-label="Folder path" aria-describedby="directory-hint" placeholder="folder path" value={path} readOnly={busy !== null} autoComplete="off" spellCheck={false}
          onChange={event => { revision.current++; completion.current = null; setPath(event.target.value); setError(''); setMissing(null); }}
          onKeyDownCapture={event => { if (event.key === 'Tab' && missing === null) { event.preventDefault(); event.stopPropagation(); if (event.shiftKey) { if (!remote) input.current?.parentElement?.querySelector('button')?.focus(); } else void complete(); } }} />
        {!remote && <button type="button" className="icon-button" aria-label="Browse folders" title="Browse folders" disabled={busy !== null} onClick={() => void open(true)}><Icon name="folder" /></button>}
      </div>
      <div id="directory-hint" className="directory-hint">{busy ? busy === 'creating' ? 'Creating folder…' : 'Checking folder…' : <>Tab complete · Enter open{remote ? ' · Remote folder' : ' · Shift+Tab browse'}</>}</div>
      {missing !== null && <div className="directory-create-confirm" role="alertdialog" aria-label="Create missing folder?">
        <div><strong>This folder doesn't exist. Create it?</strong><small>Parent folders will be created too.</small></div>
        <div className="directory-confirm-actions"><button ref={editButton} type="button" disabled={busy !== null} onClick={editPath}>Keep editing</button><button type="button" className="primary" disabled={busy !== null} onClick={() => void open(false, true)}>Create folder</button></div>
      </div>}
      {error && <div className="field-error" role="alert">{error}</div>}
    </form>
  </Modal>;
}
