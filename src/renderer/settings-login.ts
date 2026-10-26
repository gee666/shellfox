import { useEffect, useRef, useState } from 'react';
import type { ManagerApi } from '../shared/contracts';
import type { RemoteState } from '../shared/remote-contracts';
import type { RemoteController } from './remote-controller';
import { request } from './api';

type LoginState = { phase: 'idle' | 'requesting' | 'browser' | 'error' | 'cancelled'; message: string; url: string | null };
const IDLE: LoginState = { phase: 'idle', message: '', url: null };
export function useSettingsLogin(controller: RemoteController, local: ManagerApi, state: RemoteState | null) {
  const [login, setLogin] = useState<LoginState>(IDLE);
  const attempt = useRef<{ id: number; baseline: string | null; latestURL: string | null; kind: 'browser' | 'key'; opened: Set<string> } | null>(null);
  const serial = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const alive = useRef(true);
  const stateRef = useRef(state); stateRef.current = state;
  function finish() { clearTimeout(timer.current); timer.current = undefined; attempt.current = null; }
  useEffect(() => {
    alive.current = true;
    return () => { alive.current = false; serial.current++; finish(); };
  }, []);
  async function open(url: string, id: number, automatic: boolean) {
    const current = attempt.current;
    if (!current || current.id !== id) return;
    current.latestURL = url;
    if (automatic && current.opened.size > 0) {
      setLogin(previous => previous.url === url ? previous : { ...previous, url, message: 'Sign-in address updated. Continue in browser.' });
      return;
    }
    current.opened.add(url);
    setLogin({ phase: 'browser', url, message: 'Finish signing in in your browser. This page updates automatically.' });
    const result = local.openExternal ? await request(() => local.openExternal!({ url }), 10_000) : null;
    if (!alive.current || attempt.current?.id !== id || stateRef.current?.status === 'online') return;
    if (!result?.ok) setLogin({ phase: 'error', url: current.latestURL, message: 'Could not open your browser. Continue sign-in or copy the login address.' });
  }
  function accept(next: RemoteState, id: number, confirmed = false) {
    const current = attempt.current;
    if (!current || current.id !== id) return;
    if (next.status === 'online') { finish(); setLogin(IDLE); return; }
    if (!next.enabled || next.status === 'disabled') { finish(); setLogin(IDLE); return; }
    if (next.status === 'error') { const url = current.latestURL; finish(); setLogin({ phase: 'error', url, message: next.error || 'Sign-in failed. Try again.' }); return; }
    if (!next.loginUrl) { current.baseline = null; return; }
    // A delayed login reply must not replace a newer pushed browser address.
    if (confirmed && (current.opened.size > 0 || current.latestURL !== null)) return;
    if (current.kind === 'key') {
      current.latestURL = next.loginUrl;
      setLogin(previous => previous.url === next.loginUrl ? previous : { ...previous, url: next.loginUrl });
      return;
    }
    if (next.status === 'login-required' && (confirmed || next.loginUrl !== current.baseline)) void open(next.loginUrl, id, true);
  }
  useEffect(() => {
    if (state && attempt.current) accept(state, attempt.current.id);
    else if (state?.status === 'online' || !state?.enabled) setLogin(IDLE);
  }, [state]);
  async function start(authKey?: string) {
    const api = controller.api;
    if (!api || attempt.current && (login.phase === 'requesting' || login.phase === 'browser')) return;
    finish();
    const id = ++serial.current;
    attempt.current = { id, baseline: stateRef.current?.loginUrl ?? null, latestURL: null, kind: authKey ? 'key' : 'browser', opened: new Set() };
    setLogin({ phase: 'requesting', url: null, message: authKey ? 'Connecting with auth key…' : 'Starting sign-in…' });
    timer.current = setTimeout(() => {
      if (attempt.current?.id !== id) return;
      const url = attempt.current.latestURL;
      finish(); setLogin({ phase: 'error', url, message: 'Sign-in timed out. Try again or continue in your browser.' });
    }, 120_000);
    const result = await controller.run(() => api.login(authKey ? { authKey } : {}), { reportError: false });
    if (!alive.current || attempt.current?.id !== id) return;
    if (!result.ok) { const url = attempt.current.latestURL; finish(); setLogin({ phase: 'error', url, message: 'Could not start sign-in. Check your connection and try again.' }); return; }
    accept(result.value, id, true);
  }
  function cancel() { serial.current++; finish(); setLogin({ phase: 'cancelled', url: null, message: 'Sign-in cancelled.' }); }
  function retry() { finish(); void start(); }
  function continueLogin() {
    const url = attempt.current?.latestURL ?? login.url ?? (state?.status === 'login-required' ? state.loginUrl : null);
    if (!url) { retry(); return; }
    if (!attempt.current) attempt.current = { id: ++serial.current, baseline: url, latestURL: url, kind: 'browser', opened: new Set() };
    const id = attempt.current.id;
    void open(url, id, false);
    if (!timer.current) timer.current = setTimeout(() => {
      if (attempt.current?.id !== id) return;
      const latestURL = attempt.current.latestURL;
      finish(); setLogin({ phase: 'error', url: latestURL, message: 'Sign-in timed out. Try again.' });
    }, 120_000);
  }
  return { ...login, busy: login.phase === 'requesting' || login.phase === 'browser', start, cancel, retry, continueLogin };
}
