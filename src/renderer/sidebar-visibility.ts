import { useEffect, useState } from 'react';

export const SESSIONS_SIDEBAR_OPEN_KEY = 'shellfox.sessionsSidebarOpen';

export function useSessionsSidebarOpen() {
  const [open, setOpen] = useState(() => {
    try { return localStorage.getItem(SESSIONS_SIDEBAR_OPEN_KEY) !== 'false'; }
    catch { return true; }
  });
  useEffect(() => {
    try { localStorage.setItem(SESSIONS_SIDEBAR_OPEN_KEY, String(open)); }
    catch { /* The toggle still works when storage is disabled. */ }
  }, [open]);
  return [open, setOpen] as const;
}
