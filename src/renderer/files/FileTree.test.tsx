// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { failure, success, type ManagerApi } from '../../shared/contracts';
import type { FileApi } from '../../shared/file-contracts';
import { mockApi } from '../test-fixtures';
import { FileTree } from './FileTree';

beforeEach(() => vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] }));
afterEach(() => { cleanup(); vi.useRealTimers(); });
async function tick(ms = 120) { await act(async () => { vi.advanceTimersByTime(ms); }); }
async function setup() {
  const base = mockApi().api;
  base.completeDirectory.mockResolvedValue(success({ matches: ['/home/child/', '/home/other/'] }));
  const files = {
    list: vi.fn<FileApi['list']>(async input => success({ path: input.path === '~' ? '/home' : input.path, parentPath: '/', entries: [], truncated: false })),
    read: vi.fn<FileApi['read']>(async () => failure('NOT_FOUND', 'Missing file')),
    write: vi.fn<FileApi['write']>(async () => failure('UNSUPPORTED', 'Not used')),
    writeImage: vi.fn<FileApi['writeImage']>(async () => failure('UNSUPPORTED', 'Not used')),
    readAsset: vi.fn<FileApi['readAsset']>(async () => failure('UNSUPPORTED', 'Not used')),
  } satisfies FileApi;
  const createDirectory = vi.fn<NonNullable<ManagerApi['createDirectory']>>(async input => success({ cwd: input.path }));
  const api: ManagerApi = { ...base, files, createDirectory };
  const onOpen = vi.fn(), onFolderChange = vi.fn();
  const view = render(<FileTree api={api} onOpen={onOpen} onFolderChange={onFolderChange} />);
  await act(async () => {});
  const input = screen.getByRole('combobox', { name: 'Folder path' });
  const enter = async () => { await act(async () => {
    if (fireEvent.keyDown(input, { key: 'Enter' })) fireEvent.submit(input.closest('form') ?? input);
  }); };
  return { ...view, api, files, createDirectory, onOpen, onFolderChange, input, enter };
}

it('uses the shared path picker and lists host home with no session identity', async () => {
  const f = await setup();
  expect(f.input.closest('form')).toHaveClass('directory-path-picker');
  expect(f.files.list).toHaveBeenCalledExactlyOnceWith({ path: '~' });
  expect(f.input).toHaveValue('/home'); expect(f.onFolderChange).toHaveBeenLastCalledWith('/home');
  expect(f.api.getHomeDirectory).toHaveBeenCalledOnce();
});

it.each(['', '~'])('opens host home after clearing %j suggestions with Escape', async path => {
  const f = await setup(); fireEvent.focus(f.input); fireEvent.change(f.input, { target: { value: path } }); await tick();
  expect(screen.getByRole('listbox', { name: 'Matching folders' })).toBeVisible();
  fireEvent.keyDown(f.input, { key: 'Escape' }); await tick(500);
  expect(screen.queryByRole('listbox')).not.toBeInTheDocument();
  await f.enter();
  expect(f.files.list).toHaveBeenLastCalledWith({ path: '~' });
  expect(f.files.list).not.toHaveBeenCalledWith({ path: '/home/child/' });
});

it('accepts the first suggestion while open, then opens it on the next Enter', async () => {
  const f = await setup(); act(() => f.input.focus()); await tick();
  await f.enter(); expect(f.input).toHaveValue('/home/child/'); expect(f.files.list).toHaveBeenCalledOnce();
  await f.enter(); expect(f.files.list).toHaveBeenLastCalledWith({ path: '/home/child/' });
});

it('opens typed files and navigates their parent without reading or creating them', async () => {
  const f = await setup(); f.files.list.mockResolvedValueOnce(failure('NOT_FOUND', 'Not a directory'))
    .mockResolvedValueOnce(success({ path: '/other', parentPath: '/', truncated: false, entries: [{ name: 'archive.zip', path: '/other/archive.zip', kind: 'file' }] }));
  fireEvent.change(f.input, { target: { value: '/other/archive.zip' } }); await f.enter();
  expect(f.onOpen).toHaveBeenCalledExactlyOnceWith('/other/archive.zip');
  expect(f.onFolderChange).toHaveBeenLastCalledWith('/other'); expect(f.input).toHaveValue('/other');
  expect(f.files.read).not.toHaveBeenCalled(); expect(f.createDirectory).not.toHaveBeenCalled();
  expect(screen.queryByRole('alert')).not.toBeInTheDocument();
});

it('asks before creating a missing directory and keeps its text when declined', async () => {
  const f = await setup();
  f.files.list.mockResolvedValueOnce(failure('NOT_FOUND', 'Missing folder')).mockResolvedValueOnce(failure('NOT_FOUND', 'Missing parent'));
  fireEvent.change(f.input, { target: { value: '/new/child' } }); await f.enter();
  expect(screen.getByRole('alertdialog', { name: 'Create missing folder?' })).toBeVisible();
  expect(screen.getByRole('button', { name: 'Keep editing' })).toHaveFocus();
  expect(f.createDirectory).not.toHaveBeenCalled(); expect(f.onOpen).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'Keep editing' }));
  expect(f.input).toHaveValue('/new/child'); expect(f.input).toHaveFocus();
  expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
});

it('creates only after confirmation and preserves the session and tab scope', async () => {
  const f = await setup();
  f.rerender(<FileTree api={f.api} sessionId="wsl" tabId="tab" onOpen={f.onOpen} />);
  await act(async () => {});
  f.files.list.mockResolvedValueOnce(failure('NOT_FOUND', 'Missing folder'))
    .mockResolvedValueOnce(success({ path: '/home', parentPath: '/', entries: [], truncated: false }));
  fireEvent.change(f.input, { target: { value: '/home/new' } }); await f.enter();
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Create folder' })); });
  expect(f.createDirectory).toHaveBeenCalledExactlyOnceWith({ sessionId: 'wsl', tabId: 'tab', path: '/home/new' });
  expect(f.files.list).toHaveBeenLastCalledWith({ sessionId: 'wsl', tabId: 'tab', path: '/home/new' });
  expect(f.input).toHaveValue('/home/new'); expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
});

it('reports creation errors and leaves the path editable', async () => {
  const f = await setup();
  f.files.list.mockResolvedValueOnce(failure('NOT_FOUND', 'Missing folder')).mockResolvedValueOnce(failure('NOT_FOUND', 'Missing parent'));
  f.createDirectory.mockResolvedValueOnce(failure('ACCESS_DENIED', 'Cannot create here'));
  fireEvent.change(f.input, { target: { value: '/new/child' } }); await f.enter();
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Create folder' })); });
  expect(screen.getByRole('alert')).toHaveTextContent('Cannot create here'); expect(f.input).not.toHaveAttribute('readonly');
  expect(f.input).toHaveValue('/new/child'); expect(f.onOpen).not.toHaveBeenCalled();
});

it('reports folder access errors without mistaking them for file navigation', async () => {
  const f = await setup(); f.files.list.mockResolvedValueOnce(failure('ACCESS_DENIED', 'No permission'));
  fireEvent.change(f.input, { target: { value: '/private' } }); await f.enter();
  expect(screen.getByRole('alert')).toHaveTextContent('No permission'); expect(f.onOpen).not.toHaveBeenCalled();
});

it('keeps WSL operations scoped until an explicit host-home navigation', async () => {
  const f = await setup(); const onHostDirectory = vi.fn();
  f.rerender(<FileTree api={f.api} sessionId="session-wsl" tabId="tab-wsl" initialDirectory="/home/guest" onOpen={f.onOpen} onHostDirectory={onHostDirectory} />);
  await act(async () => {}); f.files.list.mockClear();
  fireEvent.change(f.input, { target: { value: '/home/guest/project' } }); await f.enter();
  expect(f.files.list).toHaveBeenLastCalledWith({ sessionId: 'session-wsl', tabId: 'tab-wsl', path: '/home/guest/project' });
  fireEvent.change(f.input, { target: { value: '' } }); await f.enter();
  expect(onHostDirectory).toHaveBeenCalledExactlyOnceWith('~');
  expect(f.files.list).not.toHaveBeenCalledWith({ sessionId: 'session-wsl', tabId: 'tab-wsl', path: '~' });
});
