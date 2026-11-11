// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { success, failure, type ManagerApi } from '../../shared/contracts';
import type { FileApi } from '../../shared/file-contracts';
import { mockApi } from '../test-fixtures';
import { FileSidebar } from './FileSidebar';

afterEach(cleanup);
function setup(remote = false, open = true) {
  const files = {
    list: vi.fn<FileApi['list']>(async () => success({ path: '/work', parentPath: '/', entries: [], truncated: false })),
    read: vi.fn<FileApi['read']>(async () => success({ path: '/work/note.txt', kind: 'text', text: 'original', size: 8, revision: 'a'.repeat(64), mimeType: 'text/plain' })),
    write: vi.fn<FileApi['write']>(), writeImage: vi.fn<FileApi['writeImage']>(), readAsset: vi.fn<FileApi['readAsset']>(),
  } satisfies FileApi;
  const openFileExplorer = vi.fn<NonNullable<ManagerApi['openFileExplorer']>>(async () => success(null));
  const api: ManagerApi = { ...mockApi().api, files, openFileExplorer };
  const props = { api, onClose: vi.fn(), sessionId: 'session', tabId: 'tab', allowNativeBrowse: !remote,
    openRequest: open ? { path: '/work/note.txt', id: 'first' } : undefined };
  return { ...render(<FileSidebar {...props} />), props, files, openFileExplorer };
}

it('has no duplicate header filename and uses one compact toolbar including wrapping', async () => {
  const f = setup(); const input = await screen.findByRole('textbox', { name: 'File contents' });
  const header = f.container.querySelector('header');
  if (!header) throw new Error('Missing file header');
  expect(header).not.toHaveTextContent('note.txt');
  expect(within(header).getAllByRole('button').map(button => button.getAttribute('aria-label'))).toEqual([
    'Toggle file browser', 'Open in system explorer', 'Maximize file', 'Close file sidebar',
  ]);
  const toolbar = screen.getByRole('toolbar', { name: 'File controls' });
  expect(toolbar.querySelector('.file-document-path')).toHaveTextContent(/^note.txt$/);
  expect(toolbar.querySelector('.file-document-path')).toHaveAttribute('title', '/work/note.txt');
  expect(within(toolbar).getAllByRole('button').map(button => button.getAttribute('aria-label'))).toEqual([
    'Copy file path', 'Save file', 'Reload file', 'Show file in system explorer', 'Wrap lines',
  ]);
  for (const button of within(toolbar).getAllByRole('button')) expect(button).toHaveAttribute('title');
  expect(f.container.querySelector('.file-code-toolbar')).toBeNull();
  fireEvent.change(input, { target: { value: 'unsaved' } });
  const wrap = within(toolbar).getByRole('button', { name: 'Wrap lines' });
  expect(wrap).toHaveAttribute('aria-pressed', 'true'); fireEvent.click(wrap);
  expect(input).toHaveAttribute('wrap', 'off'); expect(input).toHaveValue('unsaved');
  expect(within(toolbar).getByRole('button', { name: 'Save file' })).toBeEnabled();
  await act(async () => { fireEvent.click(within(header).getByRole('button', { name: 'Open in system explorer' })); });
  expect(f.openFileExplorer).toHaveBeenLastCalledWith({ sessionId: 'session', tabId: 'tab', path: '/work' });
  await act(async () => { fireEvent.click(within(toolbar).getByRole('button', { name: 'Show file in system explorer' })); });
  expect(f.openFileExplorer).toHaveBeenLastCalledWith({ sessionId: 'session', tabId: 'tab', path: '/work/note.txt' });
  expect(screen.queryByRole('button', { name: 'Upload file' })).toBeNull();
});

it('returns focus to the folders chevron when a controlled shortcut collapses the focused browser', async () => {
  const f = setup(); await screen.findByRole('textbox', { name: 'File contents' });
  const path = screen.getByRole('combobox', { name: 'Folder path' }); path.focus();
  f.rerender(<FileSidebar {...f.props} browserOpen={false} />);
  const toggle = screen.getByRole('button', { name: 'Toggle file browser' });
  expect(toggle).toHaveFocus(); expect(toggle).toHaveAttribute('title', expect.stringContaining('Ctrl+Shift+E'));
});

it('shows remote transfers in the file toolbar, never the native explorer buttons', async () => {
  setup(true); await screen.findByRole('textbox', { name: 'File contents' });
  const toolbar = screen.getByRole('toolbar', { name: 'File controls' });
  expect(within(toolbar).getByRole('button', { name: 'Upload file' })).toBeVisible();
  expect(within(toolbar).getByRole('button', { name: 'Download selected file' })).toBeVisible();
  expect(screen.queryByRole('button', { name: /system explorer/ })).toBeNull();
});

it('keeps folder uploads inline before selecting a remote file and allows fullscreen without a file', async () => {
  setup(true, false); await act(async () => {});
  const browser = screen.getByRole('region', { name: 'File browser' });
  expect(within(browser).getByRole('button', { name: 'Upload file' })).toBeVisible();
  expect(screen.queryByRole('button', { name: 'Download selected file' })).toBeNull();
  expect(screen.getByRole('button', { name: 'Maximize file' })).toBeEnabled();
});

it('keeps remote downloads available when the selected file cannot be previewed', async () => {
  const f = setup(true, false); f.files.read.mockResolvedValueOnce(failure('UNSUPPORTED', 'Too large to preview'));
  f.rerender(<FileSidebar {...f.props} openRequest={{ path: '/work/large.bin', id: 'next' }} />);
  expect(await screen.findByRole('alert')).toHaveTextContent('Too large to preview');
  const toolbar = screen.getByRole('toolbar', { name: 'File controls' });
  expect(within(toolbar).getByRole('button', { name: 'Download selected file' })).toBeVisible();
  expect(toolbar.querySelector('.file-document-path')).toHaveTextContent('large.bin');
});
