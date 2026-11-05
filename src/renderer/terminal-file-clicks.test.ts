// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { installTerminalFileClicks } from './terminal-file-clicks';

const cleanups: (() => void)[] = [];
afterEach(() => { cleanups.splice(0).forEach(cleanup => cleanup()); document.body.replaceChildren(); });
function fixture(mac = false) {
  const element = document.createElement('div'), screen = document.createElement('div');
  element.append(screen); document.body.append(element);
  let active = true, hovered: string | null = '/project/file.py:12:3';
  const open = vi.fn(), copy = vi.fn(), tui = vi.fn();
  for (const type of ['mousedown', 'mouseup', 'mousemove']) screen.addEventListener(type, tui);
  cleanups.push(installTerminalFileClicks(element, { active: () => active, hovered: () => hovered, open, copy, mac }));
  const mouse = (type: string, extra: MouseEventInit = {}) => {
    const event = new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, clientX: 10, clientY: 20, ...extra });
    screen.dispatchEvent(event); return event;
  };
  return { element, screen, open, copy, tui, mouse, active: (value: boolean) => { active = value; }, hover: (value: string | null) => { hovered = value; } };
}
describe('file clicks while a terminal app captures the mouse', () => {
  it('opens a plain path once and keeps the click out of TUI mouse reports', () => {
    const f = fixture(); f.mouse('mousedown'); f.mouse('mouseup');
    expect(f.open).toHaveBeenCalledExactlyOnceWith({ path: '/project/file.py', line: 12, column: 3 });
    expect(f.tui).not.toHaveBeenCalled();
  });
  it('opens file OSC targets with source fragments and copies the hidden URL on Ctrl+Shift click', () => {
    const f = fixture(); f.hover('file:///project/main.ts#L4C2');
    f.mouse('mousedown'); f.mouse('mouseup');
    expect(f.open).toHaveBeenCalledExactlyOnceWith({ path: '/project/main.ts', line: 4, column: 2 });
    f.mouse('mousedown', { ctrlKey: true, shiftKey: true }); f.mouse('mouseup', { ctrlKey: true, shiftKey: true });
    expect(f.copy).toHaveBeenCalledExactlyOnceWith('file:///project/main.ts#L4C2');
    expect(f.open).toHaveBeenCalledOnce();
  });
  it('leaves web links, unsafe schemes, right clicks and ordinary shell clicks to existing handlers', () => {
    const f = fixture();
    for (const uri of ['https://example.com/a.py', 'javascript:alert(1)', null]) {
      f.hover(uri); f.mouse('mousedown'); f.mouse('mouseup');
    }
    f.hover('/project/file.py'); f.mouse('mousedown', { button: 2 }); f.mouse('mouseup', { button: 2 });
    f.active(false); f.mouse('mousedown'); f.mouse('mouseup');
    expect(f.tui).toHaveBeenCalledTimes(10); expect(f.open).not.toHaveBeenCalled();
  });
  it('preserves selection modifiers, cancels drags and keeps the pressed target through a repaint', () => {
    const f = fixture(); f.mouse('mousedown', { shiftKey: true }); f.mouse('mouseup', { shiftKey: true });
    expect(f.tui).toHaveBeenCalledTimes(2); expect(f.open).not.toHaveBeenCalled();
    f.mouse('mousedown'); f.mouse('mousemove', { buttons: 1, clientX: 30 }); f.mouse('mouseup', { clientX: 30 });
    expect(f.open).not.toHaveBeenCalled();
    f.mouse('mousedown'); f.hover(null); f.mouse('mouseup');
    expect(f.open).toHaveBeenCalledExactlyOnceWith({ path: '/project/file.py', line: 12, column: 3 });
    f.hover('/another.py'); f.mouse('mousedown'); f.element.dispatchEvent(new MouseEvent('mouseleave')); f.mouse('mouseup');
    expect(f.open).toHaveBeenCalledOnce();
  });
  it('uses macOS selection/copy semantics and removes capture handlers on cleanup', () => {
    const f = fixture(true);
    f.mouse('mousedown', { altKey: true }); f.mouse('mouseup', { altKey: true });
    f.mouse('mousedown', { ctrlKey: true }); f.mouse('mouseup', { ctrlKey: true });
    expect(f.tui).toHaveBeenCalledTimes(4); expect(f.open).not.toHaveBeenCalled();
    f.mouse('mousedown', { metaKey: true, shiftKey: true }); f.mouse('mouseup', { metaKey: true, shiftKey: true });
    expect(f.copy).toHaveBeenCalledOnce(); expect(f.tui).toHaveBeenCalledTimes(4);
    cleanups.splice(0).forEach(cleanup => cleanup());
    f.mouse('mousedown'); f.mouse('mouseup'); expect(f.tui).toHaveBeenCalledTimes(6);
  });
});
