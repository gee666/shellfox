import { useEffect, useRef, useState } from 'react';
import { AppState, Platform } from 'react-native';
import { updates } from './native';

export const UPDATE_INTERVAL_MS = 60 * 60 * 1000;
export interface MobileUpdateStatus { version: string | null; message: string | null }
export const updateConstraint = (platform: string) => platform === 'android'
  ? 'Downloads automatically while Shellfox is open. Android requires your permission and installer confirmation. Updates cannot install silently or on quit.'
  : 'iOS manages updates through your distribution channel. Shellfox cannot download or install its own app updates. Enable automatic updates in that channel.';

export function useUpdates(loaded: boolean, autoUpdate: boolean): MobileUpdateStatus {
  const [status, setStatus] = useState<MobileUpdateStatus>({ version: null, message: null });
  const enabled = useRef(autoUpdate);
  enabled.current = autoUpdate;
  useEffect(() => {
    if (!loaded || Platform.OS !== 'android') return;
    let active = true, busy = false;
    const poll = async () => {
      if (!active || busy || AppState.currentState !== 'active') return;
      busy = true;
      try {
        const update = await updates.check();
        if (!active) return;
        setStatus({ version: update?.version ?? null, message: null });
        if (update && enabled.current) {
          setStatus({ version: update.version, message: 'Downloading and verifying APK...' });
          const downloaded = await updates.download(() => active && enabled.current);
          if (active) setStatus({ version: downloaded?.version ?? update.version, message: downloaded ? 'Update downloaded. Confirm installation when ready.' : null });
        }
      } catch {
        if (active) setStatus(current => ({ ...current, message: 'Could not download or check the update. Use the manual update buttons to retry.' }));
      } finally { busy = false; }
    };
    void poll();
    const timer = setInterval(() => { void poll(); }, UPDATE_INTERVAL_MS);
    const subscription = AppState.addEventListener('change', state => { if (state === 'active') void poll(); });
    return () => { active = false; clearInterval(timer); subscription.remove(); };
  }, [loaded, autoUpdate]);
  return status;
}
