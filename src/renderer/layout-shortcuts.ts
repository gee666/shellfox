import { useEffect, useRef } from 'react';

export type LayoutAction = 'sessions' | 'files' | 'folders';
const keys: Record<LayoutAction, { code: string; key: string; shift: boolean }> = {
  sessions: { code: 'KeyB', key: 'B', shift: false },
  files: { code: 'KeyB', key: 'B', shift: true },
  folders: { code: 'KeyE', key: 'E', shift: true },
};

export function isMacClient() { return /Mac/.test(navigator.userAgent); }

export function layoutShortcutHint(action: LayoutAction, mac = isMacClient()) {
  const shortcut = keys[action];
  return `${mac ? '⌘' : 'Ctrl+'}${shortcut.shift ? mac ? '⇧' : 'Shift+' : ''}${shortcut.key}`;
}

export function layoutShortcut(event: KeyboardEvent, mac = isMacClient()): LayoutAction | null {
  if (event.isComposing || event.altKey || (mac ? !event.metaKey || event.ctrlKey : !event.ctrlKey || event.metaKey)) return null;
  for (const action of ['sessions', 'files', 'folders'] as const) {
    const shortcut = keys[action];
    if (event.shiftKey === shortcut.shift && (event.code === shortcut.code || !event.code && event.key.toUpperCase() === shortcut.key)) return action;
  }
  return null;
}

/** Capture before terminal/editor handlers so app shortcuts never reach the PTY. */
export function useLayoutShortcuts(run: (action: LayoutAction) => boolean) {
  const latest = useRef(run); latest.current = run;
  useEffect(() => {
    const pressed = new Set<string>();
    const handle = (event: KeyboardEvent) => {
      const key = event.code || event.key.toUpperCase();
      if (!pressed.has(key)) {
        if (event.type !== 'keydown' || event.repeat || event.defaultPrevented) return;
        const action = layoutShortcut(event);
        if (!action) return;
        const overlay = Array.from(document.querySelectorAll('[role="dialog"], [role="alertdialog"], [role="menu"]')).some(node => !node.closest('[hidden], [inert]'));
        if (overlay || !latest.current(action)) return;
        pressed.add(key);
      }
      if (event.type === 'keyup') pressed.delete(key);
      event.preventDefault(); event.stopPropagation();
    };
    const clear = () => pressed.clear();
    const events = ['keydown', 'keypress', 'keyup'] as const;
    events.forEach(type => document.addEventListener(type, handle, true));
    window.addEventListener('blur', clear);
    return () => {
      events.forEach(type => document.removeEventListener(type, handle, true));
      window.removeEventListener('blur', clear);
    };
  }, []);
}
