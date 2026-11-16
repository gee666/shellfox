import { useState } from 'react';
import type { FileApi, FileLocation } from '../../shared/file-contracts';
import { createTextFile } from '../../shared/file-actions';

/** Names a new empty text file in the browsed folder; any extension is allowed. */
export function NewFileForm({ api, folder, onCreated, onCancel }: {
  api: FileApi; folder: FileLocation; onCreated(path: string): void; onCancel(): void;
}) {
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  async function create() {
    setBusy(true); setError('');
    try {
      const result = await createTextFile(api, folder, name.trim());
      if (result.ok) onCreated(result.value.path); else setError(result.error.message);
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not create the file.'); }
    finally { setBusy(false); }
  }
  return <form className="file-new-form" aria-label="New file" onSubmit={event => { event.preventDefault(); if (!busy) void create(); }}
    onKeyDown={event => { if (event.key === 'Escape') { event.stopPropagation(); onCancel(); } }}>
    <input aria-label="New file name" placeholder="name.txt" value={name} autoFocus spellCheck={false} readOnly={busy}
      onChange={event => { setName(event.target.value); setError(''); }} />
    <button type="submit" disabled={busy || !name.trim()}>Create</button>
    <button type="button" aria-label="Cancel new file" title="Cancel" disabled={busy} onClick={onCancel}>×</button>
    {error && <p className="file-tree-message" role="alert">{error}</p>}
  </form>;
}
