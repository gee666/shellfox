// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { useState } from 'react';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { FileCodeEditor } from './FileCodeEditor';

afterEach(cleanup);
function setup(path = 'example.ts', text = 'const word = "unchanged";\n' + 'long '.repeat(100)) {
  const changed = vi.fn(), saved = vi.fn();
  function Editor() {
    const [value, setValue] = useState(text), [wrap, setWrap] = useState(true);
    return <><button onClick={() => setWrap(current => !current)}>{wrap ? 'Unwrap' : 'Wrap'}</button>
      <FileCodeEditor path={path} text={value} wrap={wrap} request={null} onSave={saved} onChange={next => { changed(next); setValue(next); }} /></>;
  }
  const view = render(<Editor />);
  const input = screen.getByRole('textbox', { name: 'File contents' });
  if (!(input instanceof HTMLTextAreaElement)) throw new Error('Missing source input');
  return { ...view, input, changed, saved };
}
it('wraps by default and toggles without changing text, selection or dirty state', () => {
  const f = setup(); f.input.setSelectionRange(4, 8);
  expect(f.input).toHaveAttribute('wrap', 'soft');
  expect(f.input.parentElement).toHaveClass('file-code-wrap');
  fireEvent.click(screen.getByRole('button', { name: 'Unwrap' }));
  expect(f.input).toHaveAttribute('wrap', 'off'); expect(f.input.selectionStart).toBe(4); expect(f.input.selectionEnd).toBe(8);
  expect(f.input.parentElement).not.toHaveClass('file-code-wrap');
  fireEvent.click(screen.getByRole('button', { name: 'Wrap' }));
  expect(f.input).toHaveAttribute('wrap', 'soft'); expect(f.changed).not.toHaveBeenCalled();
});
it('matches the highlighted viewport to textarea scrollbar dimensions and scroll offsets', () => {
  const f = setup(); const pre = f.container.querySelector('pre');
  Object.defineProperty(f.input, 'clientWidth', { value: 303 }); Object.defineProperty(f.input, 'clientHeight', { value: 175 });
  f.input.scrollTop = 54; f.input.scrollLeft = 12; fireEvent.scroll(f.input);
  expect(pre).toHaveStyle({ width: '303px', height: '175px' });
  expect(pre?.scrollTop).toBe(54); expect(pre?.scrollLeft).toBe(12);
});
it('retains unsaved edits through wrap changes and preserves the save shortcut', () => {
  const f = setup('plain.txt');
  fireEvent.change(f.input, { target: { value: 'unsaved\n' + 'x'.repeat(500) } });
  fireEvent.click(screen.getByRole('button', { name: 'Unwrap' }));
  expect(f.input.value).toBe('unsaved\n' + 'x'.repeat(500));
  expect(f.changed).toHaveBeenCalledOnce();
  fireEvent.keyDown(f.input, { ctrlKey: true, key: 's' }); expect(f.saved).toHaveBeenCalledOnce();
});
