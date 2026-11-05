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
  const api: ManagerApi = { ...base, files };
  const onOpen = vi.fn(), onFolderChange = vi.fn();
  const view = render(<FileTree api={api} onOpen={onOpen} onFolderChange={onFolderChange} />);
  await act(async () => {});
  const input = screen.getByRole('combobox', { name: 'Folder path' });
  const enter = async () => { await act(async () => {
    if (fireEvent.keyDown(input, { key: 'Enter' })) fireEvent.submit(input.closest('form') ?? input);
  }); };
  return { ...view, api, files, onOpen, onFolderChange, input, enter };
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

it('opens typed files without replacing the actual folder used for uploads', async () => {
  const f = await setup(); f.files.list.mockResolvedValueOnce(failure('NOT_FOUND', 'Not a directory'));
  fireEvent.change(f.input, { target: { value: '/home/note.txt' } }); await f.enter();
  expect(f.onOpen).toHaveBeenCalledExactlyOnceWith('/home/note.txt');
  expect(f.onFolderChange).toHaveBeenLastCalledWith('/home');
  expect(screen.queryByRole('alert')).not.toBeInTheDocument();
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
