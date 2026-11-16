// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { act, cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { failure, success, type ManagerApi } from '../../shared/contracts';
import type { FileApi } from '../../shared/file-contracts';
import { Notifications } from '../components';
import { deferred, mockApi } from '../test-fixtures';
import { CopyFilePathButton } from './CopyFilePathButton';
import { FileSidebar } from './FileSidebar';

afterEach(cleanup);
it.each(['C:\\work space\\note.ts', '/home/guest/project/note.ts', '/srv/remote/note:12.ts'])('copies the exact canonical path %j without OS conversion or line suffixes', async path => {
  const copyText = vi.fn<ManagerApi['copyText']>(async () => success({ copied: true }));
  render(<Notifications><CopyFilePathButton api={{ copyText }} path={path} /></Notifications>);
  const button = screen.getByRole('button', { name: 'Copy file path' });
  expect(button).toHaveAttribute('title', 'Copy file path'); expect(button).toHaveTextContent(''); expect(button.querySelector('svg')).not.toBeNull();
  await userEvent.setup().click(button);
  expect(copyText).toHaveBeenCalledExactlyOnceWith({ text: path });
  expect(await screen.findByRole('status')).toHaveTextContent('Path copied');
});

it('shows clipboard failures instead of claiming the path was copied', async () => {
  const copyText = vi.fn<ManagerApi['copyText']>(async () => failure('ACCESS_DENIED', 'Clipboard unavailable'));
  render(<Notifications><CopyFilePathButton api={{ copyText }} path="/remote/file.ts" /></Notifications>);
  await userEvent.setup().click(screen.getByRole('button', { name: 'Copy file path' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Clipboard unavailable');
  expect(screen.queryByText('Path copied')).not.toBeInTheDocument();
});

it('disables copying without a bridge and prevents duplicate clipboard operations while pending', async () => {
  const pending = deferred<Awaited<ReturnType<ManagerApi['copyText']>>>();
  const copyText = vi.fn<ManagerApi['copyText']>(() => pending.promise);
  const view = render(<Notifications><CopyFilePathButton api={{}} path="/note.ts" /></Notifications>);
  expect(screen.getByRole('button', { name: 'Copy file path' })).toBeDisabled();
  view.rerender(<Notifications><CopyFilePathButton api={{ copyText }} path="/note.ts" /></Notifications>);
  const user = userEvent.setup(), button = screen.getByRole('button', { name: 'Copy file path' });
  await user.click(button); expect(button).toBeDisabled(); await user.click(button); expect(copyText).toHaveBeenCalledOnce();
  await act(async () => pending.resolve(success({ copied: true })));
  expect(button).toBeEnabled();
});

it('uses the successfully opened canonical path in the sidebar and maximized view, never the file request', async () => {
  const base = mockApi().api, path = '/home/guest/canonical name.ts';
  const files: FileApi = {
    list: async () => success({ path: '/home/guest', parentPath: '/home', entries: [], truncated: false }),
    read: async () => success({ kind: 'text', path, mimeType: 'text/plain', size: 4, revision: 'a'.repeat(64), text: 'note' }),
    write: async () => failure('UNSUPPORTED', 'Not used'), writeImage: async () => failure('UNSUPPORTED', 'Not used'),
    readAsset: async () => failure('UNSUPPORTED', 'Not used'),
  };
  const api: ManagerApi = { ...base, files };
  render(<Notifications><FileSidebar api={api} openRequest={{ path: './relative.ts', line: 27, column: 6, id: 1 }} onClose={vi.fn()} /></Notifications>);
  const button = await screen.findByRole('button', { name: 'Copy file path' });
  const user = userEvent.setup(); await user.click(button);
  expect(base.copyText).toHaveBeenCalledExactlyOnceWith({ text: path });
  await user.click(screen.getByRole('button', { name: 'Maximize file' }));
  expect(screen.getByRole('button', { name: 'Copy file path' })).toBe(button);
  await user.click(button); expect(base.copyText).toHaveBeenLastCalledWith({ text: path });
});

it('shows and copies the searched path when a relative file link is missing', async () => {
  const base = mockApi().api;
  const files: FileApi = {
    list: async input => input.path === '.' ? success({ path: '/work/project', parentPath: '/work', entries: [], truncated: false }) : failure('NOT_FOUND', 'File or directory not found.'),
    read: async () => failure('NOT_FOUND', 'File or directory not found.'), write: async () => failure('UNSUPPORTED', 'Not used'),
    writeImage: async () => failure('UNSUPPORTED', 'Not used'), readAsset: async () => failure('UNSUPPORTED', 'Not used'),
  };
  render(<Notifications><FileSidebar api={{ ...base, files }} sessionId="session" openRequest={{ path: 'tmp/../REPORT.md', id: 1 }} onClose={vi.fn()} /></Notifications>);
  const error = await screen.findByText('File or directory not found.');
  const block = error.closest('.file-open-error');
  if (!(block instanceof HTMLElement)) throw new Error('Missing centered error');
  expect(within(block).getByText('/work/project/REPORT.md')).toBeVisible();
  await userEvent.setup().click(within(block).getByRole('button', { name: 'Copy file path' }));
  expect(base.copyText).toHaveBeenCalledExactlyOnceWith({ text: '/work/project/REPORT.md' });
});
