// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import '../terminal-test-mocks';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { failure, success, type ManagerApi, type SessionDto } from '../../shared/contracts';
import type { FileApi } from '../../shared/file-contracts';
import type { FileTransfersProps } from './FileTransfers';
import { App } from '../App';
import { TerminalRegistry } from '../terminal-client';
import { deferred, mockApi, session, snapshot } from '../test-fixtures';

vi.mock('./FileTransfers', () => ({ FileTransfers: (props: FileTransfersProps) => <div data-testid="file-transfer-target" data-folder={props.location.path} data-selected={props.selectedPath} data-session={props.location.sessionId} data-tab={props.location.tabId} /> }));
afterEach(() => { cleanup(); vi.restoreAllMocks(); });
beforeEach(() => localStorage.clear());
function setup(sessions: SessionDto[] = [session()]) {
  const base = mockApi(snapshot(sessions));
  const files = {
    list: vi.fn<FileApi['list']>(async input => success({ path: input.path === '~' ? '/home' : input.path, parentPath: '/', truncated: false,
      entries: [{ name: 'note.ts', path: '/home/note.ts', kind: 'file' }] })),
    read: vi.fn<FileApi['read']>(async input => success({ kind: 'text', path: input.path, mimeType: 'text/plain', size: 4, revision: 'a'.repeat(64), text: 'note' })),
    write: vi.fn<FileApi['write']>(async input => success({ path: input.path, revision: 'b'.repeat(64), size: input.text.length })),
    writeImage: vi.fn<FileApi['writeImage']>(async () => failure('UNSUPPORTED', 'Not used')),
    readAsset: vi.fn<FileApi['readAsset']>(async () => success({ mimeType: 'text/javascript', dataUrl: 'data:text/javascript;base64,YQ==', size: 1 })),
  } satisfies FileApi;
  const api: ManagerApi = { ...base.api, files };
  render(<App api={api} />);
  return { ...base, api, files, user: userEvent.setup() };
}

it('provides an icon-only folder toggle and host-home browser without any sessions', async () => {
  const f = setup([]);
  const toggle = await screen.findByRole('button', { name: 'Files' });
  await waitFor(() => expect(toggle).toBeEnabled());
  expect(toggle).toHaveTextContent(''); expect(toggle.querySelector('svg')).not.toBeNull();
  expect(screen.queryByRole('button', { name: 'Open terminal folder' })).not.toBeInTheDocument();
  await f.user.click(toggle);
  expect(await screen.findByRole('tree', { name: 'Files' })).toBeVisible();
  expect(f.files.list).toHaveBeenCalledExactlyOnceWith({ path: '~' });
  expect(screen.getByRole('combobox', { name: 'Folder path' })).toHaveValue('/home');
  const browser = screen.getByRole('button', { name: 'Toggle file browser' });
  expect(browser).toHaveAttribute('aria-expanded', 'true'); expect(browser.querySelector('svg')).not.toBeNull();
  expect(browser).toHaveTextContent('');
  await f.user.click(browser); expect(browser).toHaveAttribute('aria-expanded', 'false');
  await f.user.click(toggle); expect(screen.getByLabelText('File sidebar')).not.toBeVisible();
  await f.user.click(toggle); expect(screen.getByLabelText('File sidebar')).toBeVisible();
});

it('starts in the active tab cwd and preserves the mounted panel and dirty file across session changes and deletion', async () => {
  const first = session(), second = session(2); first.tabs[0].cwd = '/work/first'; second.tabs[0].cwd = '/work/second';
  const f = setup([first, second]);
  await screen.findByRole('button', { name: 'Select session Session 1' });
  await f.user.click(await screen.findByRole('button', { name: 'Files' }));
  expect(f.files.list).toHaveBeenCalledExactlyOnceWith({ path: '/work/first' });
  const sidebar = screen.getByLabelText('File sidebar');
  await f.user.click(await screen.findByRole('button', { name: 'note.ts' }));
  const input = await screen.findByRole('textbox', { name: 'File contents' });
  fireEvent.change(input, { target: { value: 'keep this draft' } });
  await f.user.click(screen.getByRole('button', { name: 'Select session Session 2' }));
  expect(screen.getByLabelText('File sidebar')).toBe(sidebar); expect(input).toHaveValue('keep this draft');
  expect(f.files.list).toHaveBeenCalledOnce();
  await act(async () => f.emit({ ...snapshot([]), revision: 2 }));
  await waitFor(() => expect(screen.queryByRole('button', { name: 'Select session Session 2' })).not.toBeInTheDocument());
  expect(screen.getByLabelText('File sidebar')).toBe(sidebar); expect(input).toHaveValue('keep this draft');
  await f.user.click(screen.getByRole('button', { name: 'Save file' }));
  expect(f.files.write).toHaveBeenCalledWith({ path: '/home/note.ts', text: 'keep this draft', revision: 'a'.repeat(64) });
});

it('opens the backend-reported terminal cwd through a separate icon instead of using the launch cwd', async () => {
  const item = session(), f = setup([item]);
  await screen.findByRole('button', { name: 'Select session Session 1' });
  f.files.list.mockResolvedValueOnce(success({ path: '/current/shell/folder', parentPath: '/current/shell', entries: [], truncated: false }));
  const button = screen.getByRole('button', { name: 'Open terminal folder' });
  expect(button).toHaveTextContent(''); expect(button.querySelector('svg')).not.toBeNull();
  expect(button.querySelector('path')?.getAttribute('d')).not.toBe(screen.getByRole('button', { name: 'Files' }).querySelector('path')?.getAttribute('d'));
  await f.user.click(button);
  expect(await screen.findByRole('combobox', { name: 'Folder path' })).toHaveValue('/current/shell/folder');
  expect(f.files.list).toHaveBeenCalledExactlyOnceWith({ sessionId: item.id, tabId: item.tabs[0].id, path: '.' });
  expect(screen.getByTestId('file-transfer-target')).toHaveAttribute('data-folder', '/current/shell/folder');
});

it('returns an already-browsed or hidden panel to the terminal cwd without replacing its dirty preview', async () => {
  const item = session(), f = setup([item]);
  await screen.findByRole('button', { name: 'Select session Session 1' });
  await f.user.click(screen.getByRole('button', { name: 'Files' }));
  await f.user.click(await screen.findByRole('button', { name: 'note.ts' }));
  const editor = await screen.findByRole('textbox', { name: 'File contents' });
  fireEvent.change(editor, { target: { value: 'unsaved text stays' } });
  const path = screen.getByRole('combobox', { name: 'Folder path' });
  fireEvent.change(path, { target: { value: '/browsed/elsewhere' } }); fireEvent.submit(path.closest('form') ?? path);
  await waitFor(() => expect(screen.getByTestId('file-transfer-target')).toHaveAttribute('data-folder', '/browsed/elsewhere'));
  await f.user.click(screen.getByRole('button', { name: 'Toggle file browser' }));
  await f.user.click(screen.getByRole('button', { name: 'Files' }));
  f.files.list.mockResolvedValueOnce(success({ path: '/shell/current', parentPath: '/shell', entries: [], truncated: false }));
  await f.user.click(screen.getByRole('button', { name: 'Open terminal folder' }));
  expect(await screen.findByRole('combobox', { name: 'Folder path' })).toHaveValue('/shell/current');
  expect(screen.getByRole('button', { name: 'Toggle file browser' })).toHaveAttribute('aria-expanded', 'true');
  expect(screen.getByRole('textbox', { name: 'File contents' })).toBe(editor); expect(editor).toHaveValue('unsaved text stays');
  expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument(); expect(f.files.read).toHaveBeenCalledOnce();
  f.files.list.mockResolvedValueOnce(success({ path: '/shell/again', parentPath: '/shell', entries: [], truncated: false }));
  await f.user.click(screen.getByRole('button', { name: 'Open terminal folder' }));
  await waitFor(() => expect(screen.getByRole('combobox', { name: 'Folder path' })).toHaveValue('/shell/again'));
});

it('retains WSL context when opening and editing files in the current terminal folder', async () => {
  const item = session(); item.tabs[0].profileId = 'wsl:Ubuntu';
  const f = setup([item]); await screen.findByRole('button', { name: 'Select session Session 1' });
  f.files.list.mockResolvedValueOnce(success({ path: '/home/guest/current', parentPath: '/home/guest', truncated: false,
    entries: [{ path: '/home/guest/current/note.ts', name: 'note.ts', kind: 'file' }] }));
  await f.user.click(screen.getByRole('button', { name: 'Open terminal folder' }));
  expect(await screen.findByRole('combobox', { name: 'Folder path' })).toHaveValue('/home/guest/current');
  await f.user.click(screen.getByRole('button', { name: 'note.ts' }));
  const editor = await screen.findByRole('textbox', { name: 'File contents' });
  fireEvent.change(editor, { target: { value: 'scoped WSL edit' } });
  await f.user.click(screen.getByRole('button', { name: 'Save file' }));
  const scope = { sessionId: item.id, tabId: item.tabs[0].id };
  expect(f.files.read).toHaveBeenCalledWith({ ...scope, path: '/home/guest/current/note.ts' });
  expect(f.files.write).toHaveBeenCalledWith({ ...scope, path: '/home/guest/current/note.ts', text: 'scoped WSL edit', revision: 'a'.repeat(64) });
});

it('does not open a stale terminal folder after the user switches sessions while lookup is pending', async () => {
  const first = session(), second = session(2), f = setup([first, second]);
  await screen.findByRole('button', { name: 'Select session Session 1' });
  const pending = deferred<Awaited<ReturnType<FileApi['list']>>>(); f.files.list.mockReturnValueOnce(pending.promise);
  await f.user.click(screen.getByRole('button', { name: 'Open terminal folder' }));
  expect(screen.getByRole('button', { name: 'Open terminal folder' })).toBeDisabled();
  await f.user.click(screen.getByRole('button', { name: 'Select session Session 2' }));
  await act(async () => pending.resolve(success({ path: '/old/session', parentPath: '/old', entries: [], truncated: false })));
  expect(screen.queryByLabelText('File sidebar')).not.toBeInTheDocument();
});

it('reports terminal cwd lookup failures without opening a broken panel', async () => {
  const f = setup(); await screen.findByRole('button', { name: 'Select session Session 1' });
  f.files.list.mockResolvedValueOnce(failure('ACCESS_DENIED', 'Terminal folder is inaccessible'));
  await f.user.click(screen.getByRole('button', { name: 'Open terminal folder' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Terminal folder is inaccessible');
  expect(screen.queryByLabelText('File sidebar')).not.toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'Open terminal folder' })).toBeEnabled();
});

it('keeps a failed preview target downloadable without treating it as the upload folder', async () => {
  const f = setup([]); f.files.read.mockResolvedValueOnce(failure('UNSUPPORTED', 'File is too large for preview'));
  await f.user.click(await screen.findByRole('button', { name: 'Files' }));
  await f.user.click(await screen.findByRole('button', { name: 'note.ts' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('too large');
  expect(screen.getByTestId('file-transfer-target')).toHaveAttribute('data-folder', '/home');
  expect(screen.getByTestId('file-transfer-target')).toHaveAttribute('data-selected', '/home/note.ts');
});

it('keeps terminal-relative failed preview downloads in their original session context', async () => {
  let open: Parameters<TerminalRegistry['onFileOpen']>[0] | undefined;
  vi.spyOn(TerminalRegistry.prototype, 'onFileOpen').mockImplementation(listener => { open = listener; return () => {}; });
  const item = session(); item.tabs[0].cwd = '/work/session';
  const f = setup([item]); f.files.read.mockResolvedValue(failure('UNSUPPORTED', 'Preview unavailable'));
  await screen.findByRole('button', { name: 'Select session Session 1' });
  act(() => open?.(item.tabs[0].id, { path: './large.bin' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Preview unavailable');
  const transfers = screen.getByTestId('file-transfer-target');
  expect(transfers).toHaveAttribute('data-folder', '/work/session');
  expect(transfers).toHaveAttribute('data-selected', './large.bin');
  expect(transfers).toHaveAttribute('data-session', item.id); expect(transfers).toHaveAttribute('data-tab', item.tabs[0].id);
});

it('retains WSL session/tab identity for relative reads, canonical browsing, assets and saves, then allows switching to host home', async () => {
  let open: Parameters<TerminalRegistry['onFileOpen']>[0] | undefined;
  vi.spyOn(TerminalRegistry.prototype, 'onFileOpen').mockImplementation(listener => { open = listener; return () => {}; });
  const item = session(); item.tabs[0].cwd = '/home/guest/work'; item.tabs[0].profileId = 'wsl:Ubuntu';
  const f = setup([item]);
  f.files.read.mockResolvedValue(success({ kind: 'text', path: '/home/guest/work/index.html', mimeType: 'text/html', size: 32, revision: 'a'.repeat(64), text: '<script src="app.js"></script>' }));
  await screen.findByRole('button', { name: 'Select session Session 1' });
  await waitFor(() => expect(open).toBeDefined());
  act(() => open?.(item.tabs[0].id, { path: './index.html' }));
  expect(await screen.findByTitle('HTML preview')).toBeVisible();
  const scope = { sessionId: item.id, tabId: item.tabs[0].id };
  expect(f.files.read).toHaveBeenCalledWith({ ...scope, path: './index.html' });
  expect(f.files.list).toHaveBeenCalledWith({ ...scope, path: '/home/guest/work' });
  expect(f.files.readAsset).toHaveBeenCalledWith({ ...scope, path: '/home/guest/work/index.html', assetPath: 'app.js' });
  await f.user.click(screen.getByRole('button', { name: 'Source' }));
  fireEvent.change(screen.getByRole('textbox', { name: 'File contents' }), { target: { value: '<p>updated</p>' } });
  await f.user.click(screen.getByRole('button', { name: 'Save file' }));
  expect(f.files.write).toHaveBeenCalledWith({ ...scope, path: '/home/guest/work/index.html', text: '<p>updated</p>', revision: 'a'.repeat(64) });
  const path = screen.getByRole('combobox', { name: 'Folder path' });
  fireEvent.change(path, { target: { value: '' } }); fireEvent.submit(path.closest('form') ?? path);
  await waitFor(() => expect(f.files.list).toHaveBeenCalledWith({ path: '~' }));
  expect(screen.getByRole('combobox', { name: 'Folder path' })).toHaveValue('/home');
});
