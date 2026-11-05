import { useEffect, useRef, useState } from 'react';
import type { ManagerApi } from '../../shared/contracts';
import type { FileApi, FileList } from '../../shared/file-contracts';
import { DirectoryPathInput } from '../DirectoryPathInput';
import { request } from '../api';
import { compareFileEntries } from './file-preview-utils';
import type { FileDirectoryRequest } from './file-directory-request';

interface TreeProps {
  api: ManagerApi; sessionId?: string; tabId?: string; selectedPath?: string; initialDirectory?: string;
  onOpen(path: string): void; onFolderChange?(path: string): void; allowNativeBrowse?: boolean; hidden?: boolean;
  directoryRequest?: FileDirectoryRequest; refreshId?: number; onHostDirectory?: (path: string) => void;
}
interface RowsProps extends Omit<TreeProps, 'api'> {
  api: FileApi; path: string; depth?: number; initialListing?: FileList; onListed?(listing: FileList): void;
}
function DirectoryRows({ api, sessionId, tabId, path, selectedPath, onOpen, depth = 0, initialListing, onListed }: RowsProps) {
  const [listing, setListing] = useState<FileList | null>(initialListing ?? null);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    let active = true;
    if (initialListing && revision === 0) { onListed?.(initialListing); return; }
    setListing(null); setError(null);
    void api.list({ ...(sessionId ? { sessionId } : {}), ...(tabId ? { tabId } : {}), path }).then(result => {
      if (!active) return;
      if (!result.ok) { setError(result.error.message); return; }
      setListing(result.value); onListed?.(result.value);
    }).catch((cause: unknown) => { if (active) setError(cause instanceof Error ? cause.message : 'Could not load folder.'); });
    return () => { active = false; };
  }, [api, sessionId, tabId, path, revision, initialListing]);
  if (error) return <li className="file-tree-message" role="none"><span role="alert">{error}</span> <button onClick={() => setRevision(value => value + 1)}>Retry folder</button></li>;
  if (!listing) return <li className="file-tree-message" role="none"><span role="status">Loading folder…</span></li>;
  return <>
    {listing.entries.length === 0 && <li className="file-tree-message" role="none">Empty folder</li>}
    {[...listing.entries].sort(compareFileEntries).map(entry => {
      const directory = entry.kind === 'directory', open = expanded.has(entry.path);
      return <li key={entry.path} role="treeitem" aria-selected={selectedPath === entry.path} aria-expanded={directory ? open : undefined}>
        <button className="file-tree-row" style={{ paddingLeft: 8 + depth * 12 }} title={entry.path}
          disabled={!directory && entry.kind !== 'file' && entry.kind !== 'symlink'} onClick={() => directory
            ? setExpanded(previous => { const next = new Set(previous); if (open) next.delete(entry.path); else next.add(entry.path); return next; })
            : onOpen(entry.path)}>
          <span aria-hidden="true" className="file-tree-icon">{directory ? open ? '▾' : '▸' : entry.kind === 'symlink' ? '↗' : '·'}</span><span>{entry.name}</span>
        </button>
        {directory && open && <ul role="group"><DirectoryRows api={api} sessionId={sessionId} tabId={tabId} path={entry.path}
          selectedPath={selectedPath} onOpen={onOpen} depth={depth + 1} /></ul>}
      </li>;
    })}
    {listing.truncated && <li className="file-tree-message" role="none">Folder listing truncated. Open a subfolder or enter its path.</li>}
  </>;
}

export function FileTree({ api, sessionId, tabId, selectedPath, onOpen, onFolderChange, initialDirectory, allowNativeBrowse, hidden, directoryRequest, refreshId, onHostDirectory }: TreeProps) {
  const initial = directoryRequest?.path ?? initialDirectory ?? (sessionId ? '.' : '~');
  const [path, setPath] = useState(initial), [input, setInput] = useState(initial);
  const [parent, setParent] = useState<string | null>(null);
  const [refresh, setRefresh] = useState(0), [seed, setSeed] = useState<FileList | undefined>(directoryRequest?.listing);
  const [busy, setBusy] = useState(false), [error, setError] = useState('');
  const serial = useRef(0), edited = useRef(false);
  const currentFolder = useRef(initial);
  const folderCallback = useRef(onFolderChange); folderCallback.current = onFolderChange;
  useEffect(() => () => { serial.current++; }, []);
  const listed = (listing: FileList) => {
    currentFolder.current = listing.path; setParent(listing.parentPath); folderCallback.current?.(listing.path);
    if (!edited.current) setInput(listing.path);
  };
  const showDirectory = (listing: FileList) => {
    edited.current = false; setSeed(listing); setPath(listing.path); setInput(listing.path); setRefresh(value => value + 1);
    listed(listing);
  };
  async function navigate(value: string) {
    if (!api.files) return;
    const next = value.trim() || '~';
    if (sessionId && onHostDirectory && (next === '~' || next.startsWith('~/'))) { onHostDirectory(next); return; }
    const ticket = ++serial.current;
    setBusy(true); setError('');
    try {
      const result = await api.files.list({ ...(sessionId ? { sessionId } : {}), ...(tabId ? { tabId } : {}), path: next });
      if (ticket !== serial.current) return;
      if (!result.ok) {
        if (result.error.code === 'NOT_FOUND') onOpen(next);
        else setError(result.error.message);
        return;
      }
      showDirectory(result.value);
    } catch (cause) { if (ticket === serial.current) setError(cause instanceof Error ? cause.message : 'Could not open path.'); }
    finally { if (ticket === serial.current) setBusy(false); }
  }
  const refreshFolder = () => { edited.current = false; setSeed(undefined); setPath(currentFolder.current); setRefresh(value => value + 1); };
  const commands = useRef({ navigate, refreshFolder, showDirectory }); commands.current = { navigate, refreshFolder, showDirectory };
  useEffect(() => {
    if (!directoryRequest) return;
    if (!directoryRequest.listing) { void commands.current.navigate(directoryRequest.path); return; }
    serial.current++; setBusy(false); setError(''); commands.current.showDirectory(directoryRequest.listing);
  }, [directoryRequest?.id]);
  useEffect(() => { if (refreshId) commands.current.refreshFolder(); }, [refreshId]);
  if (!api.files) return null;
  return <section className="file-browser" aria-label="File browser">
    <DirectoryPathInput api={api} value={input} readOnly={busy} suggestionsEnabled={!hidden} onError={setError}
      onChange={value => { edited.current = true; setInput(value); setError(''); }} onSubmit={() => void navigate(input)}
      onBrowse={allowNativeBrowse ? () => { void request(() => api.chooseDirectory(), 0).then(result => {
        if (!result.ok) setError(result.error.message);
        else if (result.value) {
          if (sessionId && onHostDirectory) onHostDirectory(result.value.cwd); else void navigate(result.value.cwd);
        }
      }); } : undefined}
      leading={<button type="button" aria-label="Parent folder" title="Parent folder" disabled={parent === null || busy} onClick={() => { if (parent !== null) void navigate(parent); }}>↑</button>}
      trailing={<><button type="submit" disabled={busy}>Go</button><button type="button" aria-label="Refresh files" title="Refresh files" disabled={busy} onClick={refreshFolder}>↻</button></>} />
    {error && <p className="file-tree-message" role="alert">{error}</p>}
    <ul className="file-tree" role="tree" aria-label="Files" onKeyDown={event => {
      if (!(event.target instanceof HTMLButtonElement)) return;
      const rows = [...event.currentTarget.querySelectorAll<HTMLButtonElement>('.file-tree-row:not(:disabled)')];
      const index = rows.indexOf(event.target);
      if (index < 0) return;
      const expanded = event.target.closest('[role="treeitem"]')?.getAttribute('aria-expanded');
      if (event.key === 'ArrowDown') rows[Math.min(rows.length - 1, index + 1)]?.focus();
      else if (event.key === 'ArrowUp') rows[Math.max(0, index - 1)]?.focus();
      else if (event.key === 'Home') rows[0]?.focus();
      else if (event.key === 'End') rows.at(-1)?.focus();
      else if (event.key === 'ArrowRight' && expanded === 'false') event.target.click();
      else if (event.key === 'ArrowLeft' && expanded === 'true') event.target.click();
      else return;
      event.preventDefault();
    }}>
      <DirectoryRows key={JSON.stringify([path, refresh])} api={api.files} sessionId={sessionId} tabId={tabId} path={path}
        initialListing={seed} selectedPath={selectedPath} onOpen={onOpen} onListed={listed} />
    </ul>
  </section>;
}
