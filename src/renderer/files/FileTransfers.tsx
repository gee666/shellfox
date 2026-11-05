import { useEffect, useRef, useState } from 'react';
import type { ManagerApi } from '../../shared/contracts';
import type { FileLocation } from '../../shared/file-contracts';
import { downloadDesktopFile, hasFileTransfers, uploadDesktopFile } from './desktop-file-transfers';
import './file-transfers.css';

export interface FileTransfersProps {
  api: ManagerApi;
  location: FileLocation;
  /** Selected file request, including a target whose preview failed or exceeded its limit. */
  selectedPath?: string;
  onUploaded?: () => void;
}
interface Progress { direction: 'upload' | 'download'; name: string; offset: number; total: number; cancelled: boolean }
function TransferIcon({ upload }: { upload: boolean }) {
  return <svg width="15" height="15" viewBox="0 0 16 16" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.4">
    <path d="M2 11v3h12v-3" /><path d={upload ? 'M8 11V2M4 6l4-4 4 4' : 'M8 2v9M4 7l4 4 4-4'} />
  </svg>;
}

export function FileTransfers(props: FileTransfersProps) {
  const [progress, setProgress] = useState<Progress | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const running = useRef<AbortController | null>(null);
  const mounted = useRef(true);
  const latest = useRef(props); latest.current = props;
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; running.current?.abort(); };
  }, []);
  const available = hasFileTransfers(props.api.files) && !!props.api.desktopFiles;
  async function start(direction: 'upload' | 'download') {
    if (running.current) return;
    // Never read a different host, selection or folder from props after a dialog or network await.
    const { api, selectedPath, onUploaded } = props;
    const location = { ...props.location }, files = api.files, local = api.desktopFiles;
    if (!hasFileTransfers(files) || !local || direction === 'download' && !selectedPath) return;
    const controller = new AbortController(); running.current = controller;
    setNotice(null); setError(null);
    setProgress({ direction, name: '', offset: 0, total: 0, cancelled: false });
    let committed = false;
    try {
      const update = (offset: number, total: number, name: string) => {
        if (mounted.current && !controller.signal.aborted) setProgress({ direction, offset, total, name, cancelled: false });
      };
      const name = direction === 'upload'
        ? await uploadDesktopFile(files, local, location, controller.signal, update)
        : await downloadDesktopFile(files, local, { ...location, path: selectedPath ?? '' }, controller.signal, update);
      committed = name !== null;
      if (mounted.current && name) {
        setNotice(`${direction === 'upload' ? 'Uploaded' : 'Downloaded'} ${name}`);
        const current = latest.current;
        if (direction === 'upload' && current.api === api && current.location.sessionId === location.sessionId && current.location.tabId === location.tabId && current.location.path === location.path) onUploaded?.();
      }
    } catch (cause) {
      if (mounted.current && !controller.signal.aborted) setError(cause instanceof Error ? cause.message : 'File transfer failed.');
    } finally {
      if (running.current === controller) running.current = null;
      if (mounted.current) { setProgress(null); if (controller.signal.aborted && !committed) setNotice('Transfer cancelled'); }
    }
  }
  return <div className="file-transfers">
    <div className="file-transfer-toolbar" role="group" aria-label="File transfers">
      <button type="button" aria-label="Upload file" title={available ? 'Upload a local file to this folder' : 'File transfers are not supported by this host'} disabled={!available || !props.location.path || !!progress} onClick={() => void start('upload')}><TransferIcon upload /></button>
      <button type="button" aria-label="Download selected file" title="Download selected file" disabled={!available || !props.selectedPath || !!progress} onClick={() => void start('download')}><TransferIcon upload={false} /></button>
      {progress && <>
        <progress aria-label="File transfer progress" max={Math.max(1, progress.total)} value={progress.offset} />
        <span role="status" title={progress.name}>{progress.cancelled ? 'Cancelling…' : progress.name ? `${progress.direction === 'upload' ? 'Uploading' : 'Downloading'} ${progress.name}` : 'Choosing file…'}</span>
        <button type="button" aria-label="Cancel file transfer" title="Cancel transfer" disabled={progress.cancelled} onClick={() => { running.current?.abort(); setProgress(previous => previous ? { ...previous, cancelled: true } : null); }}>×</button>
      </>}
    </div>
    {error && <p role="alert">{error}</p>}
    {notice && <span role="status">{notice}</span>}
  </div>;
}
