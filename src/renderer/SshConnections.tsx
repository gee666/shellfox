import { useEffect, useRef, useState } from 'react';
import type { ManagerApi, Result, SshProfileDto, SshProfileInput } from '../shared/contracts';
import { request } from './api';
import { Icon } from './components';

type Editor = {
  id: string; isNew: boolean; name: string; host: string; port: string; user: string;
  password: string; hasPassword: boolean; clearPassword: boolean; keyFile: string; remoteCwd: string;
};

export function SshConnections({ api }: { api: ManagerApi }) {
  const [profiles, setProfiles] = useState<SshProfileDto[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [editor, setEditor] = useState<Editor | null>(null);
  const [removing, setRemoving] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState('');
  const [status, setStatus] = useState('');
  const [statusVisible, setStatusVisible] = useState(false);
  const locked = useRef(false);
  const root = useRef<HTMLElement>(null);
  const addButton = useRef<HTMLButtonElement>(null);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    void request(() => api.listSshProfiles()).then(result => {
      if (!alive.current) return;
      setLoaded(true);
      if (result.ok) setProfiles(result.value); else setError(result.error.message);
    });
    return () => { alive.current = false; };
  }, [api]);
  useEffect(() => {
    if (!status) return;
    setStatusVisible(true);
    const timer = setTimeout(() => setStatusVisible(false), 3000);
    return () => clearTimeout(timer);
  }, [status]);
  function restoreFocus(id?: string) {
    requestAnimationFrame(() => {
      if (!alive.current) return;
      const row = id ? Array.from(root.current?.querySelectorAll<HTMLButtonElement>('[data-ssh-id]') ?? []).find(node => node.dataset.sshId === id) : null;
      (row ?? addButton.current)?.focus();
    });
  }
  function edit(profile?: SshProfileDto) {
    if (locked.current || !loaded) return;
    setRemoving(null); setError('');
    setEditor({ id: profile?.id ?? crypto.randomUUID(), isNew: !profile, name: profile?.name ?? '', host: profile?.host ?? '', port: profile ? String(profile.port) : '', user: profile?.user ?? '', password: '', hasPassword: profile?.hasPassword ?? false, clearPassword: false, keyFile: profile?.keyFile ?? '', remoteCwd: profile?.remoteCwd ?? '' });
  }
  function cancel() {
    if (locked.current) return;
    restoreFocus(editor && !editor.isNew ? editor.id : removing ?? undefined);
    setEditor(null); setRemoving(null); setError('');
  }
  function change(patch: Partial<Editor>) { setEditor(current => current ? { ...current, ...patch } : null); setError(''); }
  async function run<T>(operation: () => Promise<Result<T>>, accept: (value: T) => void, timeoutMs?: number) {
    if (locked.current) return;
    locked.current = true; setPending(true); setError('');
    try {
      const result = await request(operation, timeoutMs);
      if (!alive.current) return;
      if (result.ok) accept(result.value); else setError(result.error.message);
    } finally { locked.current = false; if (alive.current) setPending(false); }
  }
  function save() {
    if (!editor || locked.current) return;
    const issues = [];
    if (!editor.name.trim()) issues.push('Name is required.');
    if (!editor.host.trim()) issues.push('Host is required.');
    const port = editor.port.trim() ? Number(editor.port) : 22;
    if ((editor.port.trim() && !/^\d+$/.test(editor.port.trim())) || !Number.isInteger(port) || port < 1 || port > 65535) issues.push('Port must be between 1 and 65535.');
    if (issues.length) { setError(issues.join(' ')); return; }
    const input: SshProfileInput = {
      id: editor.id, name: editor.name.trim(), host: editor.host.trim(), port, user: editor.user.trim(),
      password: editor.password || (editor.clearPassword ? null : undefined),
      keyFile: editor.keyFile || null, remoteCwd: editor.remoteCwd.trim() || null,
    };
    void run(() => api.saveSshProfile(input), list => { setProfiles(list); setEditor(null); restoreFocus(input.id); });
  }
  function remove(id: string) {
    void run(() => api.deleteSshProfile({ id }), list => { setProfiles(list); setRemoving(null); restoreFocus(); });
  }
  function importSessions() {
    setStatus(''); setStatusVisible(false);
    void run(() => api.importPuttySessions(), result => {
      setProfiles(result.profiles);
      setStatus(!result.found ? 'No PuTTY sessions found' : !result.added && !result.updated ? 'Already up to date' : `${result.added} added, ${result.updated} updated`);
    });
  }
  function form() {
    if (!editor) return null;
    const savedPassword = editor.hasPassword && !editor.clearPassword;
    const field = (name: 'name' | 'host' | 'port' | 'user' | 'remoteCwd', label: string, placeholder = label) => <input aria-label={label} title={label} placeholder={placeholder} value={editor[name]} autoFocus={name === 'name'} onChange={event => change({ [name]: event.target.value })} />;
    return <form className="ssh-form" aria-label={editor.isNew ? 'Add SSH connection' : 'Edit SSH connection'} onSubmit={event => { event.preventDefault(); save(); }} onKeyDown={event => {
      if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); cancel(); }
      if (event.key === 'Enter') { event.preventDefault(); save(); }
    }}>
      <fieldset disabled={pending}>
        <div className="ssh-form-host">{field('name', 'Name')}{field('host', 'Host')}{field('port', 'Port', '22')}</div>
        <div className="ssh-form-auth">{field('user', 'User')}<div className="ssh-password"><input aria-label="Password" title="Password. Leave empty to keep the saved password." type="password" placeholder={savedPassword ? '•••••• saved' : 'Password'} value={editor.password} onChange={event => change({ password: event.target.value })} />{savedPassword && <button type="button" className="text-button" title="Clear saved password" onClick={event => { event.currentTarget.parentElement?.querySelector('input')?.focus(); change({ password: '', clearPassword: true }); }}>clear</button>}</div><div className={`ssh-key${editor.keyFile ? ' has-key' : ''}`}><input aria-label="Key file" title={editor.keyFile || 'Private key file'} placeholder="Key file" readOnly value={editor.keyFile} /><div className="ssh-key-actions"><button type="button" className="icon-button" aria-label="Choose key file" title="Choose key file" onClick={() => void run(() => api.chooseSshKeyFile(), result => { if (result) change({ keyFile: result.path }); }, 0)}><Icon name="more" /></button>{editor.keyFile && <button type="button" className="icon-button" aria-label="Clear key file" title="Clear key file" onClick={event => { event.currentTarget.closest('.ssh-key')?.querySelector('input')?.focus(); change({ keyFile: '' }); }}><Icon name="close" /></button>}</div></div></div>
        <div className="ssh-form-footer">{field('remoteCwd', 'Remote folder', '~ (home)')}<button type="button" className="text-button" onClick={cancel}>Cancel</button><button type="submit" className="ssh-save" disabled={pending}>Save</button></div>
      </fieldset>
      {error && <small className="field-error" role="alert">{error}</small>}
    </form>;
  }
  return <section className="ssh-connections" ref={root}>
    <div className="ssh-heading"><h3>SSH connections</h3><div>{status && <span className={`ssh-import-status${statusVisible ? ' visible' : ''}`} role="status" aria-hidden={!statusVisible}>{status}</span>}<button className="text-button" disabled={!loaded || pending || !!editor || !!removing} onClick={importSessions}>Import from PuTTY</button><button className="text-button" ref={addButton} disabled={!loaded || pending} onClick={() => edit()}>Add</button></div></div>
    <div className="ssh-list">
      {editor?.isNew && form()}
      {profiles.map(profile => editor?.id === profile.id ? <div key={profile.id}>{form()}</div> : removing === profile.id ? <div className="ssh-remove" key={profile.id} onKeyDown={event => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); cancel(); } }}><span>Remove {profile.name}?</span><button className="text-button ssh-danger" autoFocus disabled={pending} onClick={() => remove(profile.id)}>Remove</button><button className="text-button" disabled={pending} onClick={cancel}>Cancel</button></div> : <div className="ssh-row" key={profile.id}>
        <button className="ssh-row-copy" data-ssh-id={profile.id} aria-label={`Edit SSH connection ${profile.name}`} disabled={pending} onClick={() => edit(profile)}><span title={profile.name}>{profile.name}</span><span className="ssh-address" title={`${profile.user ? `${profile.user}@` : ''}${profile.host}:${profile.port}`}>{profile.user && `${profile.user}@`}{profile.host}{profile.port !== 22 && `:${profile.port}`}</span><small title={profile.remoteCwd ?? undefined}>{profile.remoteCwd}</small></button>
        <div className="ssh-row-actions"><button className="icon-button" aria-label={`Edit ${profile.name}`} title="Edit" disabled={pending} onClick={() => edit(profile)}><Icon name="edit" /></button><button className="icon-button" aria-label={`Remove ${profile.name}`} title="Remove" disabled={pending} onClick={() => { setEditor(null); setError(''); setRemoving(profile.id); }}><Icon name="close" /></button></div>
      </div>)}
      {loaded && !profiles.length && !editor && <p className="ssh-empty">No connections yet.</p>}
      {!editor && error && <small className="field-error" role="alert">{error}</small>}
    </div>
    <p className="settings-hint ssh-hint">Connect from any terminal: <code className="shellfox-cli-example">shellfox ssh</code></p>
  </section>;
}
