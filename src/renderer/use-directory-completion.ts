import { useEffect, useRef, useState } from 'react';
import type { ManagerApi } from '../shared/contracts';
import { visibleDirectoryCompletions } from '../shared/directory-completion';
import { request } from './api';

const DEBOUNCE_MS = 120;

/** Live suggestions and the Tab cycle share results, but accepting never opens a session. */
export function useDirectoryCompletion(api: ManagerApi, path: string, enabled: boolean, onAccept: (path: string) => void, onError: (message: string) => void) {
  const [matches, setMatches] = useState<readonly string[]>([]);
  const [active, setActive] = useState(-1);
  const [accepted, setAccepted] = useState(false);
  const [completed, setCompleted] = useState(false);
  const [composing, setComposing] = useState(false);
  const composition = useRef(false);
  const serial = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const cycle = useRef<{ matches: readonly string[]; index: number } | null>(null);
  const visible = enabled && !composing ? visibleDirectoryCompletions(path, matches) : [];

  function cancel() {
    serial.current++;
    clearTimeout(timer.current);
  }
  function edit() {
    cancel(); cycle.current = null;
    setMatches([]); setActive(-1); setAccepted(false); setCompleted(false);
  }
  useEffect(() => {
    if (!enabled || composing || completed) return;
    const ticket = ++serial.current;
    timer.current = setTimeout(() => {
      void request(() => api.completeDirectory({ path })).then(result => {
        if (serial.current !== ticket || !result.ok) return;
        cycle.current = { matches: result.value.matches, index: -1 };
        setMatches(result.value.matches); setActive(0); setAccepted(false);
      });
    }, DEBOUNCE_MS);
    return cancel;
  }, [api, path, enabled, composing, completed]);
  useEffect(() => cancel, []);

  function accept(match: string, index: number) {
    // Keep Enter armed to open this folder, rather than querying and accepting its children.
    cancel(); setCompleted(true); setAccepted(true); setActive(index);
    onAccept(match);
  }
  async function tab() {
    if (!enabled || composition.current) return;
    cancel();
    const ticket = serial.current;
    if (!cycle.current) {
      const result = await request(() => api.completeDirectory({ path }));
      if (serial.current !== ticket) return;
      if (!result.ok) { onError(result.error.message); return; }
      if (!result.value.matches.length) { onError('No matching folders'); return; }
      cycle.current = { matches: result.value.matches, index: -1 };
      setMatches(result.value.matches);
    }
    const current = cycle.current;
    if (!current.matches.length) { onError('No matching folders'); return; }
    current.index = (current.index + 1) % current.matches.length;
    accept(current.matches[current.index]!, current.index);
    if (current.matches.length === 1) cycle.current = null;
  }
  function choose(index: number) {
    const match = visible[index];
    if (match === undefined) return;
    if (cycle.current) cycle.current.index = index;
    accept(match, index);
    if (cycle.current?.matches.length === 1) cycle.current = null;
  }
  function move(direction: 1 | -1) {
    if (!visible.length) return false;
    setActive(index => index < 0 ? direction === 1 ? 0 : visible.length - 1 : (index + direction + visible.length) % visible.length);
    setAccepted(false);
    return true;
  }
  function enter() {
    if (!visible.length || accepted) return false;
    choose(active < 0 ? 0 : active);
    return true;
  }
  function startComposition() {
    edit(); composition.current = true; setComposing(true);
  }
  function endComposition() {
    composition.current = false; setComposing(false);
  }
  return { matches: visible, active, composition, edit, cancel, tab, choose, move, enter, startComposition, endComposition };
}
