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
    </div> : <button className="update-button" disabled={!supported || pending || installing}
      title={!supported ? `${update.reason ?? 'Update manually.'} ${update.url}` : undefined}
      onClick={() => void act()}>{ready || installing ? 'install and restart' : `download v${update.latest}`}</button>}
    {(error || update.error) && <small role="alert">{error ?? update.error}</small>}
  </div>;
}
