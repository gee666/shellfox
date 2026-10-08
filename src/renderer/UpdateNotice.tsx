import { useEffect, useState } from 'react';
import type { ManagerApi, UpdateStatusDto } from '../shared/contracts';
import { request } from './api';
import { Icon } from './components';

export const UPDATE_POLL_MS = 60_000;
export const DOWNLOAD_POLL_MS = 500;
export function UpdateNotice({ api }: { api: ManagerApi }) {
  const [update, setUpdate] = useState<UpdateStatusDto | null>(null);
  const [hiddenVersion, setHiddenVersion] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const phase = update?.phase ?? 'idle';
  useEffect(() => {
    if (!api.getUpdateStatus) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      const result = await request(() => api.getUpdateStatus!());
      if (!active) return;
      if (result.ok) setUpdate(result.value);
      timer = setTimeout(() => { void poll(); }, phase === 'downloading' ? DOWNLOAD_POLL_MS : UPDATE_POLL_MS);
    };
    void poll();
    return () => { active = false; if (timer) clearTimeout(timer); };
  }, [api, phase]);
  if (!update?.available || hiddenVersion === update.latest && phase !== 'downloading' && phase !== 'ready' && phase !== 'installing') return null;
  const downloading = phase === 'downloading';
  const installing = phase === 'installing';
  const ready = phase === 'ready';
  const supported = update.supported === true && !!api.downloadUpdate && !!api.installUpdate;
  const percent = update.total ? Math.min(100, Math.floor((update.received ?? 0) / update.total * 100)) : null;
  async function act() {
    if (pending || !supported) return;
    setPending(true); setError(null);
    // Native confirmation and installer setup have no renderer deadline.
    const result = await request(() => ready ? api.installUpdate!({ confirmCloseTerminals: true }) : api.downloadUpdate!(), 0);
    if (result.ok) setUpdate(result.value); else setError(result.error.message);
    setPending(false);
  }
  return <div className="update-notice" role="status">
    <div className="update-copy">
      <span>{downloading ? `Downloading Shellfox ${update.latest}` : installing ? 'Closing terminals and starting installer…' : ready ? `Shellfox ${update.latest} is ready to install` : `Shellfox ${update.latest} is available`}</span>
      {downloading && <><progress aria-label="Update download" max={update.total ?? undefined} value={update.total ? update.received ?? 0 : undefined} /><small>{percent === null ? 'Downloading…' : `${percent}%`} · {((update.received ?? 0) / 1024 / 1024).toFixed(1)} MB{update.total ? ` / ${(update.total / 1024 / 1024).toFixed(1)} MB` : ''}</small></>}
      {ready && <small>Installing closes all Shellfox terminals. Running commands will stop.</small>}
      {!supported && !downloading && !ready && <small>{update.reason ?? 'Update this copy manually from the releases page.'} {update.url}</small>}
      {(error || update.error) && <small role="alert">{error ?? update.error}</small>}
      {!downloading && !installing && <button disabled={!supported || pending} onClick={() => void act()}>{pending ? 'Please wait…' : ready ? 'Install and restart' : phase === 'error' ? 'Retry download' : 'Download update'}</button>}
    </div>
    {!downloading && !ready && !installing && !pending && <button className="icon-button" aria-label="Hide update notice" title="Hide this version until next start" onClick={() => setHiddenVersion(update.latest)}><Icon name="close" /></button>}
  </div>;
}
