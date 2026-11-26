import { useCallback, useEffect, useRef, useState } from 'react';

// App documents load in well under a second. A WebKit content process that
// never starts leaves the view blank without any error, so a load that takes
// this long is retried in a new WebView.
export const WEBVIEW_LOAD_TIMEOUT_MS = 10_000;
const ATTEMPTS = 3;

export type WebViewLoadState = 'loading' | 'loaded' | 'failed';
/** Why the last attempt ended: no load in time, a lost content process, or a load error. */
export type WebViewFailure = { cause: 'timeout' } | { cause: 'stopped' } | { cause: 'error'; message: string };

/**
 * Replaces a WebView that has not loaded `source` in time, up to three times
 * per document, then calls `onGiveUp`. Mount the WebView with `key` and spread
 * `handlers` on it. `crash` says what a lost content process does: 'replace'
 * reloads the document, 'give-up' keeps the caller's own recovery.
 */
export function useRecoveringWebView(source: unknown, { onGiveUp, crash, active = true }: {
  onGiveUp: (failure: WebViewFailure) => void;
  crash: 'replace' | 'give-up';
  /** False while the caller shows no WebView, so nothing can time out. */
  active?: boolean;
}) {
  const giveUp = useRef(onGiveUp);
  giveUp.current = onGiveUp;
  const [state, setState] = useState<WebViewLoadState>('loading');
  const [key, setKey] = useState(0);
  // Bounded per document: a page that kills its process cannot reload forever.
  const attempts = useRef(1);
  const fail = useCallback((failure: WebViewFailure) => { setState('failed'); giveUp.current(failure); }, []);
  const replace = useCallback((failure: WebViewFailure) => {
    if (attempts.current >= ATTEMPTS) { fail(failure); return; }
    attempts.current += 1;
    setState('loading'); setKey(value => value + 1);
  }, [fail]);
  const retry = useCallback(() => { attempts.current = 1; setState('loading'); setKey(value => value + 1); }, []);
  useEffect(() => { attempts.current = 1; setState('loading'); }, [source]);
  useEffect(() => {
    if (!active || state !== 'loading') return;
    const timer = setTimeout(() => replace({ cause: 'timeout' }), WEBVIEW_LOAD_TIMEOUT_MS);
    return () => clearTimeout(timer);
  }, [active, state, key, source, replace]);
  const stopped = useCallback(() => {
    if (crash === 'replace') replace({ cause: 'stopped' }); else fail({ cause: 'stopped' });
  }, [crash, replace, fail]);
  return {
    state, key, retry, replace,
    handlers: {
      // onLoad, not onLoadEnd: onLoadEnd also follows a failed load.
      onLoad: () => setState('loaded'),
      onContentProcessDidTerminate: stopped,
      onRenderProcessGone: stopped,
    },
  };
}
