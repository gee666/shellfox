import { useCallback, useEffect, useRef, useState } from 'react';
import { emptyModifiers, type TerminalModifiers } from './protocol';

/** Arm indefinitely; after input, keep the chord available for a short repeat window. */
export function useStickyModifiers(delayMs: number, send: (value: TerminalModifiers) => void) {
  const [modifiers, setModifiers] = useState<TerminalModifiers>(emptyModifiers);
  const current = useRef<TerminalModifiers>(emptyModifiers);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const options = useRef({ delayMs, send });
  options.current = { delayMs, send };
  const cancel = useCallback(() => {
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = null;
  }, []);
  const update = useCallback((value: TerminalModifiers) => {
    current.current = value;
    setModifiers(value);
    options.current.send(value);
  }, []);
  const reset = useCallback(() => { cancel(); update(emptyModifiers); }, [cancel, update]);
  const activity = useCallback(() => {
    cancel();
    if (Object.values(current.current).some(Boolean)) {
      timer.current = setTimeout(reset, options.current.delayMs);
    }
  }, [cancel, reset]);
  const toggle = useCallback((key: keyof TerminalModifiers) => {
    cancel();
    update({ ...current.current, [key]: !current.current[key] });
  }, [cancel, update]);
  useEffect(() => {
    if (timer.current !== null) activity();
  }, [delayMs, activity]);
  useEffect(() => cancel, [cancel]);
  return { modifiers, current, toggle, activity, reset };
}
