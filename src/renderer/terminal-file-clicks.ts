import { parseTerminalFileTarget, type TerminalFileTarget } from '../shared/terminal-file-links';

/** File links belong to Shellfox even while a TUI owns terminal mouse reports. */
export function installTerminalFileClicks(element: HTMLElement, options: {
  active(): boolean;
  hovered(): string | null;
  open(target: TerminalFileTarget): void;
  copy?(uri: string): void;
  mac: boolean;
}): () => void {
  let press: { uri: string; target: TerminalFileTarget; x: number; y: number; copy: boolean; moved: boolean } | null = null;
  const consume = (event: MouseEvent) => { event.preventDefault(); event.stopPropagation(); };
  const down = (event: MouseEvent) => {
    if (!options.active() || event.button !== 0 || options.mac && event.ctrlKey) return;
    const uri = options.hovered(), target = uri ? parseTerminalFileTarget(uri) : null;
    if (!uri || !target) return;
    const copy = event.shiftKey && (options.mac ? event.metaKey : event.ctrlKey);
    // Keep the native selection modifier available for selecting terminal text.
    if (!copy && (options.mac ? event.altKey : event.shiftKey)) return;
    press = { uri, target, x: event.clientX, y: event.clientY, copy, moved: false };
    consume(event);
  };
  const move = (event: MouseEvent) => {
    if (!press) return;
    if (Math.hypot(event.clientX - press.x, event.clientY - press.y) > 4) press.moved = true;
    consume(event);
  };
  const up = (event: MouseEvent) => {
    if (!press || event.button !== 0) return;
    const selected = press; press = null;
    consume(event);
    // Terminal output can clear hover during the press. Keep the clicked target,
    // not the replacement row painted by a streaming agent.
    if (selected.moved || Math.hypot(event.clientX - selected.x, event.clientY - selected.y) > 4) return;
    if (selected.copy) options.copy?.(selected.uri);
    else options.open(selected.target);
  };
  const clear = () => { press = null; };
  element.addEventListener('mousedown', down, true);
  element.addEventListener('mousemove', move, true);
  element.addEventListener('mouseup', up, true);
  element.addEventListener('mouseleave', clear);
  element.ownerDocument.defaultView?.addEventListener('blur', clear);
  return () => {
    clear();
    element.removeEventListener('mousedown', down, true);
    element.removeEventListener('mousemove', move, true);
    element.removeEventListener('mouseup', up, true);
    element.removeEventListener('mouseleave', clear);
    element.ownerDocument.defaultView?.removeEventListener('blur', clear);
  };
}
