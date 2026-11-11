// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import './terminal-test-mocks';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { failure, success, type ManagerApi } from '../shared/contracts';
import type { FileApi } from '../shared/file-contracts';
import { App } from './App';
import { mockApi, snapshot } from './test-fixtures';
import { SESSIONS_SIDEBAR_OPEN_KEY } from './sidebar-visibility';
import { SIDEBAR_WIDTH_KEY } from './sidebar-width';
import { layoutShortcutHint } from './layout-shortcuts';

vi.mock('./files/FileTransfers', () => ({ FileTransfers: () => null }));
afterEach(() => { cleanup(); vi.restoreAllMocks(); });
beforeEach(() => localStorage.clear());

async function ready(api: ManagerApi = mockApi().api) {
  const view = render(<App api={api} />);
  await screen.findByRole('tab', { name: /Shell 1/ });
  return { ...view, user: userEvent.setup() };
}
function shortcut(code: string, shiftKey = false) {
  const options = { code, ctrlKey: true, shiftKey, bubbles: true, cancelable: true };
  fireEvent(document, new KeyboardEvent('keydown', options));
  fireEvent(document, new KeyboardEvent('keyup', options));
}

it('collapses sessions without replacing the terminal and restores its saved width and navigation', async () => {
  localStorage.setItem(SIDEBAR_WIDTH_KEY, '284');
  const { user, container, unmount } = await ready();
  const terminal = screen.getByLabelText('Interactive terminal');
  const hide = screen.getByRole('button', { name: 'Hide sessions' });
  expect(hide).toHaveAttribute('title', `Hide sessions (${layoutShortcutHint('sessions')})`);
  await user.click(hide);
  expect(screen.getByRole('button', { name: 'Show sessions' })).toHaveFocus();
  expect(screen.queryByRole('button', { name: 'Select session Session 1' })).not.toBeInTheDocument();
  expect(screen.queryByRole('button', { name: 'Settings' })).not.toBeInTheDocument();
  expect(screen.queryByRole('separator', { name: 'Sidebar width' })).not.toBeInTheDocument();
  expect(container.querySelector('.app-shell')).toHaveClass('sessions-collapsed');
  expect(screen.getByLabelText('Interactive terminal')).toBe(terminal);
  expect(localStorage.getItem(SESSIONS_SIDEBAR_OPEN_KEY)).toBe('false');
  await user.click(screen.getByRole('button', { name: 'Show sessions' }));
  expect(screen.getByRole('separator', { name: 'Sidebar width' })).toHaveAttribute('aria-valuenow', '284');
  expect(screen.getByRole('button', { name: 'Select session Session 1' })).toBeVisible();
  expect(screen.getByLabelText('Interactive terminal')).toBe(terminal);
  await user.click(screen.getByRole('button', { name: 'Hide sessions' })); unmount();
  await ready(); expect(screen.getByRole('button', { name: 'Show sessions' })).toBeVisible();
  expect(screen.queryByRole('button', { name: 'Select session Session 1' })).not.toBeInTheDocument();
});

it('handles the sessions shortcut while typing and viewing settings, without dismissing dialogs', async () => {
  const { user } = await ready();
  const row = screen.getByRole('button', { name: 'Select session Session 1' }); row.focus();
  shortcut('KeyB'); expect(screen.getByRole('button', { name: 'Show sessions' })).toHaveFocus();
  shortcut('KeyB'); await user.click(screen.getByRole('button', { name: 'Settings' }));
  shortcut('KeyB'); expect(screen.getByRole('region', { name: 'Settings' })).toBeVisible();
  shortcut('KeyB'); await user.click(screen.getByRole('button', { name: 'New session' }));
  expect(screen.getByRole('dialog')).toBeVisible();
  shortcut('KeyB'); expect(screen.getByRole('button', { name: 'Hide sessions' })).toBeVisible();
});

it('uses a safe default for corrupt persisted visibility and still toggles when writes fail', async () => {
  localStorage.setItem(SESSIONS_SIDEBAR_OPEN_KEY, 'not-a-boolean');
  vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new Error('Storage disabled'); });
  const { user } = await ready();
  await user.click(screen.getByRole('button', { name: 'Hide sessions' }));
  expect(screen.getByRole('button', { name: 'Show sessions' })).toBeVisible();
});

it('toggles the filesystem and its top folders, retaining drafts while sessions are collapsed', async () => {
  const base = mockApi(snapshot([]));
  const files: FileApi = {
    list: vi.fn(async () => success({ path: '/work', parentPath: '/', truncated: false, entries: [{ name: 'note.txt', path: '/work/note.txt', kind: 'file' as const }] })),
    read: vi.fn(async () => success({ kind: 'text' as const, path: '/work/note.txt', mimeType: 'text/plain', size: 4, revision: 'a'.repeat(64), text: 'note' })),
    write: vi.fn(async () => failure('UNSUPPORTED', 'Not used')),
    writeImage: vi.fn(async () => failure('UNSUPPORTED', 'Not used')),
    readAsset: vi.fn(async () => failure('UNSUPPORTED', 'Not used')),
  };
  const user = userEvent.setup(); render(<App api={{ ...base.api, files }} />);
  await waitFor(() => expect(screen.getByRole('button', { name: 'Files' })).toBeEnabled());
  expect(screen.getByRole('button', { name: 'Files' })).toHaveAttribute('title', `Files (${layoutShortcutHint('files')})`);
  shortcut('KeyE', true); expect(screen.queryByLabelText('File sidebar')).not.toBeInTheDocument();
  shortcut('KeyB', true); await user.click(await screen.findByRole('button', { name: 'note.txt' }));
  const editor = await screen.findByRole('textbox', { name: 'File contents' });
  fireEvent.change(editor, { target: { value: 'unsaved draft' } }); editor.focus();
  shortcut('KeyE', true); expect(screen.getByRole('button', { name: 'Toggle file browser' })).toHaveAttribute('aria-expanded', 'false');
  shortcut('KeyB'); expect(screen.getByRole('button', { name: 'Show sessions' })).toBeVisible();
  expect(screen.getByRole('textbox', { name: 'File contents' })).toBe(editor);
  shortcut('KeyB', true); expect(screen.getByLabelText('File sidebar')).not.toBeVisible();
  await waitFor(() => expect(screen.getByRole('button', { name: 'Files' })).toHaveFocus());
  shortcut('KeyB', true); expect(screen.getByRole('textbox', { name: 'File contents' })).toBe(editor);
  expect(editor).toHaveValue('unsaved draft');
  expect(screen.getByRole('button', { name: 'Toggle file browser' })).toHaveAttribute('aria-expanded', 'false');
  shortcut('KeyE', true); expect(screen.getByRole('button', { name: 'Toggle file browser' })).toHaveAttribute('aria-expanded', 'true');
});
