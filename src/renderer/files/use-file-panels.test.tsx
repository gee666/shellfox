// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { failure, success, type ManagerApi } from '../../shared/contracts';
import type { FileApi } from '../../shared/file-contracts';
import { mockApi } from '../test-fixtures';
import { useFilePanels } from './use-file-panels';

afterEach(cleanup);
function Harness({ api, onToggle }: { api: ManagerApi; onToggle?: (handled: boolean) => void }) {
  const panels = useFilePanels();
  return <><button onClick={() => panels.toggle({ hostId: 'peer-1', api, directory: '/work' })}>Files</button>
    <button onClick={() => onToggle?.(panels.toggleBrowser('peer-1'))}>Folders</button>{panels.render('peer-1')}</>;
}

it('toggles controlled folder visibility only in an open host panel and retains it across closing', async () => {
  const onToggle = vi.fn();
  const api: ManagerApi = { ...mockApi().api, files: {
    list: async () => success({ path: '/work', parentPath: '/', entries: [], truncated: false }),
    read: vi.fn(), write: vi.fn(), writeImage: vi.fn(), readAsset: vi.fn(),
  } };
  render(<Harness api={api} onToggle={onToggle} />);
  fireEvent.click(screen.getByRole('button', { name: 'Folders' })); expect(onToggle).toHaveBeenLastCalledWith(false);
  fireEvent.click(screen.getByRole('button', { name: 'Files' }));
  expect(await screen.findByRole('region', { name: 'File browser' })).toBeVisible();
  fireEvent.click(screen.getByRole('button', { name: 'Folders' })); expect(onToggle).toHaveBeenLastCalledWith(true);
  expect(screen.queryByRole('region', { name: 'File browser' })).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'Files' }));
  fireEvent.click(screen.getByRole('button', { name: 'Folders' })); expect(onToggle).toHaveBeenLastCalledWith(false);
  fireEvent.click(screen.getByRole('button', { name: 'Files' }));
  expect(screen.getByRole('button', { name: 'Toggle file browser' })).toHaveAttribute('aria-expanded', 'false');
  fireEvent.click(screen.getByRole('button', { name: 'Folders' }));
  expect(await screen.findByRole('region', { name: 'File browser' })).toBeVisible();
});
it('retains the panel API and dirty draft when reconnect replaces a wrapper for the same host', async () => {
  const firstWriter = vi.fn<FileApi['write']>(async input => success({ path: input.path, size: input.text.length, revision: 'b'.repeat(64) }));
  const nextWriter = vi.fn<FileApi['write']>(async input => success({ path: input.path, size: input.text.length, revision: 'c'.repeat(64) }));
  let activeWriter = firstWriter;
  const list = vi.fn<FileApi['list']>(async () => success({ path: '/work', parentPath: '/', truncated: false, entries: [{ path: '/work/note.ts', name: 'note.ts', kind: 'file' }] }));
  const read = vi.fn<FileApi['read']>(async () => success({ path: '/work/note.ts', kind: 'text', mimeType: 'text/plain', size: 4, text: 'note', revision: 'a'.repeat(64) }));
  function wrapper(): ManagerApi {
    // Like createRemoteManagerApi, each wrapper delegates to the host's current connection.
    const files: FileApi = {
      list, read, write: input => activeWriter(input),
      writeImage: async () => failure('UNSUPPORTED', 'Not used'), readAsset: async () => failure('UNSUPPORTED', 'Not used'),
    };
    return { ...mockApi().api, files };
  }
  const user = userEvent.setup(), first = wrapper();
  const view = render(<Harness api={first} />);
  await user.click(screen.getByRole('button', { name: 'Files' }));
  await user.click(await screen.findByRole('button', { name: 'note.ts' }));
  const input = await screen.findByRole('textbox', { name: 'File contents' });
  fireEvent.change(input, { target: { value: 'retained through reconnect' } });
  activeWriter = nextWriter;
  view.rerender(<Harness api={wrapper()} />);
  await user.click(screen.getByRole('button', { name: 'Files' }));
  expect(screen.getByLabelText('File sidebar')).not.toBeVisible();
  await user.click(screen.getByRole('button', { name: 'Files' }));
  expect(screen.getByRole('textbox', { name: 'File contents' })).toBe(input);
  expect(input).toHaveValue('retained through reconnect'); expect(read).toHaveBeenCalledOnce();
  await user.click(screen.getByRole('button', { name: 'Save file' }));
  expect(nextWriter).toHaveBeenCalledWith({ path: '/work/note.ts', text: 'retained through reconnect', revision: 'a'.repeat(64) });
  expect(firstWriter).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: 'Close file sidebar' }));
  expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
});
