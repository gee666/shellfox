// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { success, type ManagerApi } from '../../shared/contracts';
import type { FileApi } from '../../shared/file-contracts';
import { mockApi } from '../test-fixtures';
import { FileSidebar } from './FileSidebar';
import { FILE_BROWSER_HEIGHT_KEY, FILE_WIDTH_KEY } from './use-file-layout';

function viewport(width: number, height = 800) {
  Object.defineProperty(window, 'innerWidth', { configurable: true, value: width });
  Object.defineProperty(window, 'innerHeight', { configurable: true, value: height });
  window.dispatchEvent(new Event('resize'));
}
beforeEach(() => { localStorage.clear(); viewport(1200); });
afterEach(() => { cleanup(); vi.restoreAllMocks(); viewport(1024, 768); });
function setup(html = false) {
  const path = html ? '/work/index.html' : '/work/note.txt', text = html ? '<input value="original"><button>Count</button>' : 'original';
  const files = {
    list: vi.fn<FileApi['list']>(async () => success({ path: '/work', parentPath: '/', truncated: false, entries: [] })),
    read: vi.fn<FileApi['read']>(async () => success({ kind: 'text', path, text, revision: 'a'.repeat(64), mimeType: html ? 'text/html' : 'text/plain', size: text.length })),
    write: vi.fn<FileApi['write']>(async input => success({ path, revision: 'b'.repeat(64), size: input.text.length })),
    writeImage: vi.fn<FileApi['writeImage']>(), readAsset: vi.fn<FileApi['readAsset']>(),
  } satisfies FileApi;
  const api: ManagerApi = { ...mockApi().api, files };
  const props = { api, onClose: vi.fn(), openRequest: { path, id: 1 } };
  const view = render(<FileSidebar {...props} />);
  return { ...view, props, files };
}
function drag(handle: HTMLElement, from: number, to: number, horizontal = false, end = 'pointerup') {
  let captured = false;
  Object.assign(handle, {
    setPointerCapture: vi.fn(() => { captured = true; }), hasPointerCapture: vi.fn(() => captured), releasePointerCapture: vi.fn(() => { captured = false; }),
  });
  for (const [type, coordinate] of [['pointerdown', from], ['pointermove', to], [end, to]] as const) {
    const event = new MouseEvent(type, { bubbles: true, button: 0, ...(horizontal ? { clientY: coordinate } : { clientX: coordinate }) });
    Object.defineProperty(event, 'pointerId', { value: 7 }); fireEvent(handle, event);
  }
}

describe('file sidebar layout', () => {
  it('resizes both panels by keyboard and pointer, clamps and resets persisted sizes', async () => {
    setup(); await screen.findByRole('textbox', { name: 'File contents' });
    const width = screen.getByRole('separator', { name: 'File sidebar width' });
    const height = screen.getByRole('separator', { name: 'File browser height' });
    expect(width).toHaveAttribute('aria-orientation', 'vertical'); expect(height).toHaveAttribute('aria-orientation', 'horizontal');
    fireEvent.keyDown(width, { key: 'ArrowLeft' }); expect(width).toHaveAttribute('aria-valuenow', '424');
    drag(width, 800, 720); expect(width).toHaveAttribute('aria-valuenow', '504');
    expect(localStorage.getItem(FILE_WIDTH_KEY)).toBe('504');
    drag(width, 720, 2000); expect(width).toHaveAttribute('aria-valuenow', '300');
    drag(width, 720, -2000); expect(width).toHaveAttribute('aria-valuenow', '776');
    fireEvent.doubleClick(width); expect(width).toHaveAttribute('aria-valuenow', '420');
    fireEvent.keyDown(height, { key: 'ArrowDown', shiftKey: true }); expect(height).toHaveAttribute('aria-valuenow', '252');
    drag(height, 250, 310, true); expect(height).toHaveAttribute('aria-valuenow', '312');
    expect(localStorage.getItem(FILE_BROWSER_HEIGHT_KEY)).toBe('312');
    fireEvent.keyDown(height, { key: 'Home' }); expect(height).toHaveAttribute('aria-valuenow', '220');
    fireEvent.doubleClick(height); expect(localStorage.getItem(FILE_BROWSER_HEIGHT_KEY)).toBe('220');
    expect(document.querySelector('.layout-resize-shield')).toBeNull();
  });

  it('preserves the exact dirty editor, selection and saved layout through maximize and restore', async () => {
    const f = setup(); const editor = await screen.findByRole('textbox', { name: 'File contents' });
    fireEvent.change(editor, { target: { value: 'unsaved draft' } });
    if (!(editor instanceof HTMLTextAreaElement)) throw new Error('Expected textarea');
    editor.setSelectionRange(2, 7); editor.scrollTop = 20;
    fireEvent.keyDown(screen.getByRole('separator', { name: 'File sidebar width' }), { key: 'ArrowLeft' });
    fireEvent.keyDown(screen.getByRole('separator', { name: 'File browser height' }), { key: 'ArrowDown' });
    const sidebar = screen.getByLabelText('File sidebar'), style = sidebar.getAttribute('style');
    fireEvent.click(screen.getByRole('button', { name: 'Maximize file' }));
    expect(sidebar).toHaveAttribute('data-maximized', 'true');
    expect(screen.getByRole('button', { name: 'Toggle file browser' })).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByRole('region', { name: 'File browser' })).toBeNull();
    expect(screen.queryByRole('separator', { name: 'File sidebar width' })).toBeNull();
    expect(screen.getByRole('textbox', { name: 'File contents' })).toBe(editor);
    expect(editor).toHaveValue('unsaved draft'); expect(editor.selectionStart).toBe(2); expect(editor.selectionEnd).toBe(7); expect(editor.scrollTop).toBe(20);
    fireEvent.click(screen.getByRole('button', { name: 'Restore file' }));
    expect(sidebar).toHaveAttribute('data-maximized', 'false'); expect(sidebar).toHaveAttribute('style', style);
    expect(screen.getByRole('button', { name: 'Toggle file browser' })).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByRole('textbox', { name: 'File contents' })).toBe(editor);
    expect(screen.queryByRole('alertdialog')).toBeNull(); expect(f.files.read).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole('button', { name: 'Save file' }));
    expect(f.files.write).toHaveBeenCalledWith({ path: '/work/note.txt', text: 'unsaved draft', revision: 'a'.repeat(64) });
  });

  it('does not recreate or renavigate an interactive HTML iframe when sizing or maximizing', async () => {
    const f = setup(true); const frame = await screen.findByTitle('HTML preview');
    const source = frame.getAttribute('src'); frame.setAttribute('data-interactive-state', 'retained');
    fireEvent.keyDown(screen.getByRole('separator', { name: 'File browser height' }), { key: 'ArrowUp' });
    fireEvent.click(screen.getByRole('button', { name: 'Maximize file' }));
    fireEvent.click(screen.getByRole('button', { name: 'Restore file' }));
    expect(screen.getByTitle('HTML preview')).toBe(frame); expect(frame).toHaveAttribute('src', source);
    expect(frame).toHaveAttribute('data-interactive-state', 'retained'); expect(f.files.read).toHaveBeenCalledOnce();
  });

  it('temporarily clamps on small windows without overwriting desktop preferences', async () => {
    setup(); await screen.findByRole('textbox', { name: 'File contents' });
    const width = screen.getByRole('separator', { name: 'File sidebar width' });
    drag(width, 800, 620); const sidebar = screen.getByLabelText('File sidebar');
    expect(localStorage.getItem(FILE_WIDTH_KEY)).toBe('600');
    act(() => viewport(700, 350)); expect(sidebar).toHaveAttribute('data-narrow', 'true');
    expect(screen.queryByRole('separator', { name: 'File sidebar width' })).toBeNull();
    expect(localStorage.getItem(FILE_WIDTH_KEY)).toBe('600');
    act(() => viewport(1200)); expect(sidebar).toHaveAttribute('data-narrow', 'false');
    expect(screen.getByRole('separator', { name: 'File sidebar width' })).toHaveAttribute('aria-valuenow', '600');
    expect(screen.getByRole('separator', { name: 'File browser height' })).toHaveAttribute('aria-valuenow', '220');
  });

  it('keeps explicit browser changes and folder requests made while maximized', async () => {
    const f = setup(); await screen.findByRole('textbox', { name: 'File contents' });
    const toggle = screen.getByRole('button', { name: 'Toggle file browser' });
    fireEvent.click(toggle);
    fireEvent.click(screen.getByRole('button', { name: 'Maximize file' }));
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    f.rerender(<FileSidebar {...f.props} directoryRequest={{ path: '/requested', id: 'next', listing: { path: '/requested', parentPath: '/', truncated: false, entries: [] } }} />);
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    fireEvent.click(screen.getByRole('button', { name: 'Restore file' }));
    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    fireEvent.click(screen.getByRole('button', { name: 'Maximize file' }));
    fireEvent.click(toggle); fireEvent.click(toggle);
    fireEvent.click(screen.getByRole('button', { name: 'Restore file' }));
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
  });

  it('hides the split handle with the browser and ends cancelled drags', async () => {
    setup(); await screen.findByRole('textbox', { name: 'File contents' });
    drag(screen.getByRole('separator', { name: 'File browser height' }), 200, 240, true, 'pointercancel');
    expect(document.querySelector('.layout-resize-shield')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Toggle file browser' }));
    expect(screen.queryByRole('separator', { name: 'File browser height' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Toggle file browser' }));
    expect(screen.getByRole('separator', { name: 'File browser height' })).toHaveAttribute('aria-valuenow', '260');
  });
});
