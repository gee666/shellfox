// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { failure, success, type ManagerApi, type Result } from '../shared/contracts';
import { DirectoryPrompt } from './DirectoryPrompt';
import { deferred, mockApi } from './test-fixtures';
afterEach(cleanup);
async function setup(remote = false) {
  const { api } = mockApi(); api.resolveDirectory.mockResolvedValue(failure('NOT_FOUND', 'Folder is missing.'));
  const createDirectory = vi.fn(async (_input: { path: string }) => success({ cwd: '/home/user/new/child' }));
  const bridge: ManagerApi = { ...api, createDirectory };
  const create = vi.fn(async (_cwd: string) => true), close = vi.fn();
  const view = render(<DirectoryPrompt api={bridge} remote={remote} machineName={remote ? 'Build server' : 'Local'} onCreate={create} onClose={close} />);
  const input = screen.getByRole('combobox', { name: 'Folder path' });
  fireEvent.change(input, { target: { value: '~/new/child' } });
  await act(async () => { fireEvent.submit(input.closest('form')!); });
  return { ...view, api, bridge, input, createDirectory, create, close };
}
it('offers recursive creation only after NOT_FOUND and makes Keep editing the safe default', async () => {
  const f = await setup();
  expect(screen.getByRole('alertdialog', { name: 'Create missing folder?' })).toBeVisible();
  expect(screen.getByRole('button', { name: 'Keep editing' })).toHaveFocus();
  expect(f.createDirectory).not.toHaveBeenCalled(); expect(f.create).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole('button', { name: 'Keep editing' }));
  expect(f.input).toHaveValue('~/new/child'); expect(f.input).toHaveFocus();
  expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument(); expect(f.close).not.toHaveBeenCalled();
  expect(f.createDirectory).not.toHaveBeenCalled();
});
it('creates the exact typed path only after confirmation, then opens its canonical returned directory', async () => {
  const f = await setup(true);
  expect(screen.getByText('Build server')).toBeVisible(); expect(screen.queryByRole('button', { name: 'Browse folders' })).not.toBeInTheDocument();
  await userEvent.click(screen.getByRole('button', { name: 'Create folder' }));
  expect(f.createDirectory).toHaveBeenCalledExactlyOnceWith({ path: '~/new/child' });
  expect(f.create).toHaveBeenCalledExactlyOnceWith('/home/user/new/child'); expect(f.close).toHaveBeenCalledOnce();
});
it('lets Escape decline folder creation without closing the path editor', async () => {
  const f = await setup(), user = userEvent.setup();
  await user.keyboard('{Escape}');
  expect(f.input).toHaveValue('~/new/child'); expect(f.input).toHaveFocus(); expect(f.close).not.toHaveBeenCalled();
  expect(f.createDirectory).not.toHaveBeenCalled();
  await user.keyboard('{Escape}'); expect(f.close).toHaveBeenCalledOnce();
});
it('does not create folders for permission errors or paths that are files', async () => {
  const f = await setup(); await userEvent.click(screen.getByRole('button', { name: 'Keep editing' }));
  f.api.resolveDirectory.mockResolvedValueOnce(failure('VALIDATION', 'This path is not a directory.'));
  await act(async () => { fireEvent.submit(f.input.closest('form')!); });
  expect(screen.getByRole('alert')).toHaveTextContent('not a directory');
  expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument(); expect(f.createDirectory).not.toHaveBeenCalled();
});
it('locks duplicate create attempts and never opens a session after unmount', async () => {
  const f = await setup(), pending = deferred<Result<{ cwd: string }>>();
  f.createDirectory.mockImplementationOnce(async () => pending.promise);
  const button = screen.getByRole('button', { name: 'Create folder' });
  fireEvent.click(button); fireEvent.click(button);
  expect(f.createDirectory).toHaveBeenCalledOnce(); expect(button).toBeDisabled(); expect(f.input).toHaveAttribute('readonly');
  f.unmount(); await act(async () => { pending.resolve(success({ cwd: '/created' })); });
  expect(f.create).not.toHaveBeenCalled(); expect(f.close).not.toHaveBeenCalled();
});
it('keeps the path editable after a failed creation with no automatic retry', async () => {
  const f = await setup(); f.createDirectory.mockResolvedValueOnce(failure('AUTH_FAILED', 'Permission denied.'));
  await userEvent.click(screen.getByRole('button', { name: 'Create folder' }));
  expect(screen.getByRole('alert')).toHaveTextContent('Permission denied'); expect(f.input).toHaveValue('~/new/child'); expect(f.input).toHaveFocus();
  expect(f.createDirectory).toHaveBeenCalledOnce(); expect(f.create).not.toHaveBeenCalled(); expect(f.close).not.toHaveBeenCalled();
});
