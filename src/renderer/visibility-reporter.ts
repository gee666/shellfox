import type { ManagerApi } from '../shared/contracts';
import { request } from './api';

const reporters = new WeakMap<ManagerApi, (sessionIds: string[]) => void>();

/** Share one transport lane across hook cleanup/remount and keep only the latest set. */
export function visibilityReporter(api: ManagerApi): (sessionIds: string[]) => void {
  const existing = reporters.get(api);
  if (existing) return existing;
  let last: string[] | undefined;
  let queued: string[] | undefined;
  let sending = false;
  async function drain() {
    if (sending) return;
    sending = true;
    try {
      while (queued) {
        const sessionIds = queued; queued = undefined;
        // A display timeout must not free the lane while the host is still working.
        // The transport owns its timeout; background failures remain quiet.
        await request(() => api.setVisibleSessions({ sessionIds }), 0);
      }
    } finally { sending = false; }
  }
  const report = (sessionIds: string[]) => {
    if (last && last.length === sessionIds.length && last.every((id, index) => id === sessionIds[index])) return;
    last = [...sessionIds]; queued = last;
    void drain();
  };
  reporters.set(api, report);
  return report;
}
