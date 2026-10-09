import { memo, useCallback, useEffect, useRef, useSyncExternalStore } from 'react';
import { useNotify } from './components';
import { TerminalLoading } from './TerminalPlaceholder';
import type { TerminalRegistry } from './terminal-client';

export const TerminalViewport = memo(function TerminalViewport({ registry, tabId, generation, visible }: { registry: TerminalRegistry; tabId: string; generation?: string; visible: boolean }) {
  const host = useRef<HTMLDivElement>(null);
  const notify = useNotify();
  const state = useSyncExternalStore(
    useCallback(listener => registry.subscribe(tabId, listener), [registry, tabId]),
    useCallback(() => registry.getState(tabId), [registry, tabId]),
  );
  useEffect(() => {
    if (visible && state.operationError) {
      notify(state.operationError);
      registry.clearOperationError(tabId, state.operationError);
    }
  }, [state.operationError, visible, notify, registry, tabId]);
  useEffect(() => registry.mount(tabId, host.current!, visible), [registry, tabId]);
  useEffect(() => {
    const current = registry.getState(tabId);
    // Mount already requests the authoritative generation. Do not request a
    // second replay while that initial attachment is still in flight.
    if (generation && current.generation && current.generation !== generation) registry.refresh(tabId);
  }, [registry, tabId, generation]);
  useEffect(() => {
    registry.visibility(tabId, visible);
    if (!visible) return;
    let frame: number | null = null;
    const schedule = () => {
      if (frame !== null) return;
      frame = requestAnimationFrame(() => { frame = null; registry.fit(tabId); });
    };
    const observer = new ResizeObserver(schedule);
    observer.observe(host.current!);
    schedule();
    registry.focus(tabId);
    let active = true;
    void document.fonts?.ready.then(() => { if (active) schedule(); });
    return () => { active = false; if (frame !== null) cancelAnimationFrame(frame); observer.disconnect(); };
  }, [registry, tabId, visible]);
  return <section className="terminal-viewport" aria-label="Terminal viewport">
    {visible && state.phase === 'unavailable' && <button className="text-button terminal-reconnect" onClick={() => registry.refresh(tabId)}>Reconnect terminal</button>}
    {visible && state.phase === 'connecting' && <div className="terminal-loading-overlay"><TerminalLoading /></div>}
    {visible && state.phase === 'unavailable' && state.error && <span className="terminal-buffer-note" role="alert">{state.error.message}</span>}
    {!state.error && state.warning && <span className="terminal-buffer-note" title={state.warning}>Earlier output unavailable</span>}
    <div className="terminal-host" ref={host} aria-label="Interactive terminal" />
  </section>;
});
