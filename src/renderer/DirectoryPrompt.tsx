import { useEffect, useRef, useState } from 'react';
import type { ManagerApi } from '../shared/contracts';
import { request } from './api';
import { Icon, Modal } from './components';
import { DirectoryPathInput } from './DirectoryPathInput';

export function DirectoryPrompt({ api, onCreate, onClose, remote = false, machineName }: {
  api: ManagerApi; onCreate(cwd: string): Promise<boolean>; onClose(): void; remote?: boolean; machineName?: string;
}) {
  const [path, setPath] = useState('');
  const [error, setError] = useState('');
  const [missing, setMissing] = useState<string | null>(null);
  const [busy, setBusy] = useState<'checking' | 'creating' | null>(null);
  const input = useRef<HTMLInputElement>(null), editButton = useRef<HTMLButtonElement>(null);
  const alive = useRef(true), locked = useRef(false);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
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
    locked.current = true; setBusy(createMissing ? 'creating' : 'checking'); setError('');
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
  return <Modal title="New session" className="directory-modal" initialFocus="input" onClose={close}>
    <div className="directory-form">
      {machineName && <div className="directory-machine"><Icon name={remote ? 'server' : 'monitor'} /><span>{machineName}</span>{remote && <small>Remote folder</small>}</div>}
      <DirectoryPathInput api={api} value={path} inputRef={input} readOnly={busy !== null} suggestionsEnabled={missing === null}
        onChange={value => { setPath(value); setError(''); setMissing(null); }} onError={setError} onSubmit={() => void open(false)}
        onBrowse={remote ? undefined : () => void open(true)}
        hint={busy ? busy === 'creating' ? 'Creating folder…' : 'Checking folder…' : <>Tab complete · ↑↓ select · Enter accept/open{remote ? ' · Remote folder' : ' · Shift+Tab browse'}</>} />
      {missing !== null && <div className="directory-create-confirm" role="alertdialog" aria-label="Create missing folder?">
        <div><strong>This folder doesn't exist. Create it?</strong><small>Parent folders will be created too.</small></div>
        <div className="directory-confirm-actions"><button ref={editButton} type="button" disabled={busy !== null} onClick={editPath}>Keep editing</button><button type="button" className="primary" disabled={busy !== null} onClick={() => void open(false, true)}>Create folder</button></div>
      </div>}
      {error && <div className="field-error" role="alert">{error}</div>}
    </div>
  </Modal>;
}
