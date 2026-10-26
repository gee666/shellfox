import { useCallback, useEffect, useRef, useState } from 'react';
import { Linking } from 'react-native';
import type { ShellfoxModel } from './useShellfox';

type LoginModel = Pick<ShellfoxModel, 'network' | 'login' | 'report'>;
type Request = { id: number; url: string | null };

export function useBrowserLogin({ network, login, report }: LoginModel) {
  const [request, setRequest] = useState<Request | null>(null);
  const current = useRef(0);
  const launched = useRef(0);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; ++current.current; };
  }, []);
  const finish = useCallback((id: number, error?: unknown) => {
    if (!mounted.current || current.current !== id) return;
    if (error) report(error);
    setRequest(null);
    ++current.current;
  }, [report]);
  const start = () => {
    const id = ++current.current;
    setRequest({ id, url: network.loginUrl });
    if (!network.loginUrl) {
      void login().then(status => {
        if (!mounted.current || current.current !== id) return;
        if (status.error) { finish(id, new Error(status.error)); return; }
        if (status.status === 'online') { finish(id); return; }
        setRequest(value => value?.id === id ? { id, url: status.loginUrl } : value);
      }).catch(error => finish(id, error));
    }
  };
  const id = request?.id;
  useEffect(() => {
    if (id === undefined) return;
    const timer = setTimeout(() => finish(id, new Error('Sign-in timed out. Try again.')), 30000);
    return () => clearTimeout(timer);
  }, [id, finish]);
  useEffect(() => {
    if (!request || launched.current === request.id) return;
    if (network.error) { finish(request.id, new Error(network.error)); return; }
    if (network.status === 'online') { finish(request.id); return; }
    const raw = request.url ?? network.loginUrl;
    if (!raw) return; // Native login publishes AuthURL asynchronously through polling.
    launched.current = request.id;
    void (async () => {
      const url = new URL(raw);
      if (url.origin !== 'https://login.tailscale.com' || url.username || url.password)
        throw new Error('Invalid Tailscale sign-in URL.');
      // An explicit user request must not depend on a stale AppState snapshot.
      await Linking.openURL(url.href);
    })().then(() => finish(request.id), error => finish(request.id, error));
  }, [request, network.loginUrl, network.status, network.error, finish]);
  return { pending: request !== null, start };
}
