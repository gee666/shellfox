import { useEffect, useRef, useState } from 'react';
import type { ManagerApi, UpdateStatusDto } from '../shared/contracts';
import { request } from './api';

export const UPDATE_POLL_MS = 60_000;
export const DOWNLOAD_POLL_MS = 500;
export function UpdateNotice({ api }: { api: ManagerApi }) {
  const [update, setUpdate] = useState<UpdateStatusDto | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [eta, setEta] = useState<string | null>(null);
  const sample = useRef<{ version: string | null; time: number; received: number } | null>(null);
  const phase = update?.phase ?? 'idle';
  useEffect(() => {
    if (!api.getUpdateStatus) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      const result = await request(() => api.getUpdateStatus!());
      if (!active) return;
      if (result.ok) setUpdate({ ...result.value });
      timer = setTimeout(() => { void poll(); }, phase === 'downloading' ? DOWNLOAD_POLL_MS : UPDATE_POLL_MS);
    };
    void poll();
    return () => { active = false; if (timer) clearTimeout(timer); };
  }, [api, phase]);
  useEffect(() => {
    if (phase !== 'downloading' || !update) { sample.current = null; setEta(null); return; }
    const received = update.received ?? 0, now = Date.now(), previous = sample.current;
    if (!previous || previous.version !== update.latest || received <= previous.received || !update.total) {
      sample.current = { version: update.latest, time: now, received }; setEta(null); return;
    }
    const elapsed = now - previous.time;
    if (elapsed <= 0) return;
    const seconds = Math.max(0, Math.ceil((update.total - received) * elapsed / (received - previous.received) / 1000));
    setEta(seconds < 60 ? `${seconds}s` : `${Math.ceil(seconds / 60)}m`);
    sample.current = { version: update.latest, time: now, received };
  }, [update, phase]);
  if (!update?.available) return null;
  const downloading = phase === 'downloading';
  const installing = phase === 'installing';
  const ready = phase === 'ready';
  const supported = update.supported === true && !!api.downloadUpdate && !!api.installUpdate;
  async function act() {
    if (pending || !supported) return;
    setPending(true); setError(null);
    // Native confirmation and installer setup have no renderer deadline.
    const result = await request(() => ready ? api.installUpdate!({ confirmCloseTerminals: true }) : api.downloadUpdate!(), 0);
    if (result.ok) setUpdate(result.value); else setError(result.error.message);
    setPending(false);
  }
  return <div className="update-notice" role="status">
    {downloading ? <div className="update-progress">
      <progress aria-label="Update download" max={update.total ?? undefined} value={update.total ? update.received ?? 0 : undefined} />
      <small className="update-eta" aria-label="Estimated time remaining" title="Estimated time remaining">{eta ?? '…'}</small>
    </div> : ready || installing ? <button className="update-button" disabled={!supported || pending || installing}
      title="Also installs automatically when you close Shellfox"
      onClick={() => void act()}>
      <span>install and restart</span>
      <svg className="update-symbol" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M19.5 10a8 8 0 1 0-.8 6M19.5 4.5V10H14" />
      </svg>
    </button> : <div className="update-available">
      <span><span className="update-prompt">{'>.'}</span>new version available</span>
      <button className="update-download" aria-label={`Download v${update.latest}`} disabled={!supported || pending}
        title={!supported ? `${update.reason ?? 'Update manually.'} ${update.url}` : `Download v${update.latest}`}
        onClick={() => void act()}>
        <svg className="update-symbol" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <circle cx="12" cy="12" r="8" />
          <path d="M12 8v8m-3-3 3 3 3-3" />
        </svg>
      </button>
    </div>}
    {(error || update.error) && <small role="alert">{error ?? update.error}</small>}
  </div>;
}
