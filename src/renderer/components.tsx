import { createContext, useCallback, useContext, useEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { createPortal } from 'react-dom';
import type { AppError, Result, SessionDto, TabDto } from '../shared/contracts';
import { request } from './api';

export function Icon({ name }: { name: 'plus' | 'settings' | 'close' | 'chevron' | 'pin' | 'folder' | 'edit' | 'more' }) {
  const paths = { more: 'M3 8h.01M8 8h.01M13 8h.01', edit: 'm10 2 4 4-8 8H2v-4l8-8ZM8.5 3.5l4 4', folder: 'M2 4h5l1.5 2H14v7H2V4Zm0 0V2.5h4L7.5 4H13v2', pin: 'M9.5 2 14 6.5l-2.25.75-2.5 2.5.25 3L5.25 8.5l3-.25 2.5-2.5L9.5 2ZM6.75 9.25 2.5 13.5', plus: 'M8 3v10M3 8h10', close: 'm4 4 8 8M12 4l-8 8', chevron: 'm6 3 5 5-5 5',
    settings: 'm6 2 .5-1h3L10 2l1.5 1 1.5-.2 1.5 2.6-.8 1.3v1.6l.8 1.3-1.5 2.6-1.5-.2-1.5 1-.5 1h-3L6 13l-1.5-1-1.5.2L1.5 9.6l.8-1.3V6.7l-.8-1.3L3 2.8l1.5.2L6 2Z' };
  return <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.25" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={paths[name]} />{name === 'settings' && <circle cx="8" cy="7.5" r="2.25" />}</svg>;
}
export type DotState = { kind: 'error' | 'busy' | 'running' | 'shell'; title: string };
export function tabDot(tab: TabDto, busy = false): DotState {
  if (tab.status === 'error' || tab.error) return { kind: 'error', title: tab.error ? `Error: ${tab.error.message}` : 'Error' };
  if (tab.status === 'running') return { kind: busy ? 'busy' : 'running', title: busy ? 'Agent working' : 'Agent idle' };
  return { kind: 'shell', title: 'Shell' };
}
export function sessionDot(session: SessionDto, busy: Record<string, boolean>): DotState {
  if (session.error) return { kind: 'error', title: `Error: ${session.error.message}` };
  const priority = { error: 3, busy: 2, running: 1, shell: 0 };
  return session.tabs.filter(tab => tab.lifecycle !== 'closed').map(tab => tabDot(tab, busy[tab.id]))
    .reduce<DotState>((best, dot) => priority[dot.kind] > priority[best.kind] ? dot : best, { kind: 'shell', title: 'Shell' });
}
export function StatusDot({ state }: { state: DotState }) {
  return <span className={`status-dot dot-${state.kind}`} aria-label={state.title} />;
}

type ShellfoxNotice = AppError | { message: string; tone: 'info'; durationMs?: number };
const NotificationContext = createContext<(notice: ShellfoxNotice) => void>(() => {});
export const useNotify = () => useContext(NotificationContext);
export function Notifications({ children }: { children: ReactNode }) {
  const [messages, setMessages] = useState<{ id: number; message: string; info: boolean }[]>([]);
  const timers = useRef(new Set<ReturnType<typeof setTimeout>>());
  const serial = useRef(0);
  const recent = useRef(new Map<string, number>());
  useEffect(() => () => timers.current.forEach(clearTimeout), []);
  const notify = useCallback((notice: ShellfoxNotice) => {
    const info = 'tone' in notice && notice.tone === 'info';
    const duration = ('durationMs' in notice ? notice.durationMs : undefined) ?? (info ? 2000 : 6000);
    const key = JSON.stringify([info ? 'info' : 'code' in notice ? notice.code : 'error', notice.message]);
    const now = Date.now();
    for (const [key, expires] of recent.current) if (expires <= now) recent.current.delete(key);
    if (recent.current.has(key)) return;
    // Do not extend the timer on repeats, or immediately recreate a dismissed toast.
    // A later user attempt can report the same failure once this short window expires.
    recent.current.set(key, now + duration);
    const id = ++serial.current;
    setMessages(items => [...items.slice(-3), { message: notice.message, info, id }]);
    const timer = setTimeout(() => { setMessages(items => items.filter(item => item.id !== id)); timers.current.delete(timer); }, duration);
    timers.current.add(timer);
  }, []);
  return <NotificationContext.Provider value={notify}>{children}<div className="toasts" aria-live="polite">{messages.map(message => <div className={`toast${message.info ? ' shellfox-toast-info' : ''}`} role={message.info ? 'status' : 'alert'} key={message.id}><span>{message.message}</span><button className="icon-button" aria-label="Dismiss message" onClick={() => setMessages(items => items.filter(item => item.id !== message.id))}><Icon name="close" /></button></div>)}</div></NotificationContext.Provider>;
}
export function useAction() {
  const [pending, setPending] = useState(false);
  const locked = useRef(false);
  const mounted = useRef(true);
  const notify = useNotify();
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  /** timeoutMs: 0 waits for user-interactive operations (native dialogs) without a transport deadline. */
  async function run<T>(operation: () => Promise<Result<T>>, options: { timeoutMs?: number } = {}): Promise<Result<T> | undefined> {
    if (locked.current) return;
    locked.current = true; setPending(true);
    try {
      const result = await request(operation, options.timeoutMs);
      if (!result.ok && result.error.code !== 'SETTLE_CONFIRM_REQUIRED') notify(result.error);
      if (!mounted.current) return;
      return result;
    } finally { locked.current = false; if (mounted.current) setPending(false); }
  }
  return { pending, run };
}

export function Modal({ title, children, onClose, className = '', initialFocus }: { title: string; children: ReactNode; onClose: () => void; className?: string; initialFocus?: string }) {
  const root = useRef<HTMLDivElement>(null);
  const close = useRef(onClose); close.current = onClose;
  useEffect(() => {
    const prior = document.activeElement as HTMLElement | null;
    const node = root.current!;
    const focusables = () => Array.from(node.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), summary, [tabindex="0"]')).filter(element => element.getClientRects().length > 0);
    (node.querySelector<HTMLElement>(initialFocus ?? 'button:not(:disabled)') ?? node).focus();
    const keydown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') { event.preventDefault(); close.current(); }
      if (event.key !== 'Tab') return;
      const elements = focusables(), first = elements[0], last = elements.at(-1);
      if (!first) { event.preventDefault(); node.focus(); }
      else if (event.shiftKey && (document.activeElement === first || document.activeElement === node)) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
    };
    node.addEventListener('keydown', keydown);
    return () => { node.removeEventListener('keydown', keydown); if (prior?.isConnected) prior.focus(); };
  }, []);
  return <div className="modal-backdrop" onMouseDown={event => { if (event.target === event.currentTarget) onClose(); }}><div className={`modal ${className}`} role="dialog" aria-modal="true" aria-label={title} ref={root} tabIndex={-1}><header><h2>{title}</h2><button className="icon-button" aria-label="Close dialog" onClick={onClose}><Icon name="close" /></button></header>{children}</div></div>;
}

export function ContextMenu({ x, y, children, onClose }: { x: number; y: number; children: ReactNode; onClose: () => void }) {
  const root = useRef<HTMLDivElement>(null);
  const close = useRef(onClose); close.current = onClose;
  useEffect(() => {
    const node = root.current!;
    const prior = document.activeElement as HTMLElement | null;
    const rect = node.getBoundingClientRect();
    node.style.left = `${Math.max(4, Math.min(x, window.innerWidth - rect.width - 4))}px`;
    node.style.top = `${Math.max(4, Math.min(y, window.innerHeight - rect.height - 4))}px`;
    node.querySelector<HTMLElement>('button:not(:disabled)')?.focus();
    const dismiss = (event: PointerEvent) => { if (!node.contains(event.target as Node)) close.current(); };
    const key = (event: KeyboardEvent) => {
      if (event.key === 'Escape' || event.key === 'Tab') { close.current(); return; }
      if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
      event.preventDefault();
      const items = Array.from(node.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'));
      const current = items.indexOf(document.activeElement as HTMLButtonElement);
      const index = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : (current + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
      items[index]?.focus();
    };
    document.addEventListener('pointerdown', dismiss); document.addEventListener('keydown', key);
    return () => { document.removeEventListener('pointerdown', dismiss); document.removeEventListener('keydown', key); if (prior?.isConnected) prior.focus(); };
  }, [x, y]);
  return createPortal(<div ref={root} role="menu" className="context-menu" style={{ left: x, top: y }}>{children}</div>, document.body);
}
