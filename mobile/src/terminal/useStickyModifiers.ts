import { useCallback, useEffect, useRef, useState } from 'react';
import { emptyModifiers, type TerminalModifiers } from './protocol';

const MODIFIER_RELEASE_DELAY_MS = 430;

/** Arm indefinitely; after input, keep the chord available for a short repeat window. */
export function useStickyModifiers(send: (value: TerminalModifiers) => void) {
  const [modifiers, setModifiers] = useState<TerminalModifiers>(emptyModifiers);
  const current = useRef<TerminalModifiers>(emptyModifiers);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const sendRef = useRef(send);
  sendRef.current = send;
  const cancel = useCallback(() => {
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = null;
  }, []);
  const update = useCallback((value: TerminalModifiers) => {
    current.current = value;
    setModifiers(value);
    sendRef.current(value);
  }, []);
  const reset = useCallback(() => { cancel(); update(emptyModifiers); }, [cancel, update]);
  const activity = useCallback(() => {
    cancel();
    if (Object.values(current.current).some(Boolean)) {
      timer.current = setTimeout(reset, MODIFIER_RELEASE_DELAY_MS);
    }
  }, [cancel, reset]);
  const toggle = useCallback((key: keyof TerminalModifiers) => {
    cancel();
    update({ ...current.current, [key]: !current.current[key] });
  }, [cancel, update]);
  useEffect(() => cancel, [cancel]);
  return { modifiers, current, toggle, activity, reset };
}
