// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import './terminal-test-mocks';
import { randomUUID } from 'node:crypto';
import { afterEach, expect, it, vi } from 'vitest';
import { act, cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { CloseConfirmation, CloseConfirmationApi, Result } from '../shared/contracts';
import { failure, success } from '../shared/contracts';
import { CloseConfirmationModal } from './CloseConfirmationModal';
import { App } from './App';
import { deferred, mockApi } from './test-fixtures';

afterEach(cleanup);
function fixture() {
  const listeners = new Set<(confirmation: CloseConfirmation | null) => void>();
  const read = vi.fn<CloseConfirmationApi['getCloseConfirmation']>(async () => success(null));
  const respond = vi.fn<CloseConfirmationApi['respondCloseConfirmation']>(async () => success({ accepted: true }));
  const stop = vi.fn();
  const api = { ...mockApi().api, getCloseConfirmation: read, respondCloseConfirmation: respond,
    subscribeCloseConfirmation: (listener: (confirmation: CloseConfirmation | null) => void) => { listeners.add(listener); return () => { listeners.delete(listener); stop(); }; } };
  const show = (kind: CloseConfirmation['kind'] = 'quit', terminalCount = 2) => {
    const confirmation = { id: randomUUID(), kind, terminalCount };
    act(() => listeners.forEach(listener => listener(confirmation)));
    return confirmation;
  };
  return { api, read, respond, stop, show, user: userEvent.setup() };
}
it.each(['win32', 'linux', 'darwin'])('shows a themed in-app quit modal on %s with Cancel focused', async platform => {
  const f = fixture(); const initial = await f.api.getSnapshot();
  if (initial.ok) initial.value.probe.platform = platform;
  const view = render(<App api={f.api} />);
  await screen.findByRole('button', { name: 'Local · 1' });
  const confirmation = f.show();
  expect(screen.getByRole('dialog', { name: 'Quit Shellfox?' })).toHaveClass('close-app-modal');
  expect(screen.getByRole('button', { name: 'Cancel' })).toHaveFocus();
  expect(screen.getByRole('dialog')).toHaveTextContent('Close 2 shells and quit?');
  await f.user.click(screen.getByRole('button', { name: 'Close shells and quit' }));
  expect(f.respond).toHaveBeenCalledExactlyOnceWith({ id: confirmation.id, approved: true });
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  view.unmount(); expect(f.stop).toHaveBeenCalledOnce();
});
it.each(['Cancel', 'Escape', 'Close dialog'])('cancels with %s and leaves the application open', async action => {
  const f = fixture(); render(<CloseConfirmationModal api={f.api} />); const confirmation = f.show();
  if (action === 'Escape') await f.user.keyboard('{Escape}');
  else await f.user.click(screen.getByRole('button', { name: action }));
  expect(f.respond).toHaveBeenCalledExactlyOnceWith({ id: confirmation.id, approved: false });
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
});
it('confirms update shutdown with the same modal and handles idle updates', async () => {
  const f = fixture(); render(<CloseConfirmationModal api={f.api} />); const confirmation = f.show('update', 0);
  expect(screen.getByRole('dialog', { name: 'Install update?' })).toHaveTextContent('Install the update and restart Shellfox?');
  await f.user.click(screen.getByRole('button', { name: 'Close shells and install' }));
  expect(f.respond).toHaveBeenCalledWith({ id: confirmation.id, approved: true });
});
it('does not replace a new notification with an older initial read', async () => {
  const f = fixture(), pending = deferred<Result<CloseConfirmation | null>>(); f.read.mockReturnValueOnce(pending.promise);
  render(<CloseConfirmationModal api={f.api} />); f.show();
  await act(async () => pending.resolve(success(null)));
  expect(screen.getByRole('dialog', { name: 'Quit Shellfox?' })).toBeVisible();
});
it('keeps failed approval visible for retry and prevents duplicate responses while pending', async () => {
  const f = fixture(); render(<CloseConfirmationModal api={f.api} />); f.show();
  const pending = deferred<Result<{ accepted: true }>>(); f.respond.mockReturnValueOnce(pending.promise);
  await f.user.click(screen.getByRole('button', { name: 'Close shells and quit' }));
  expect(screen.getByRole('button', { name: 'Cancel' })).toBeDisabled();
  await f.user.keyboard('{Escape}'); expect(f.respond).toHaveBeenCalledOnce();
  await act(async () => pending.resolve(failure('INTERNAL', 'Try again.')));
  expect(screen.getByRole('alert')).toHaveTextContent('Try again.');
  await f.user.click(screen.getByRole('button', { name: 'Cancel' }));
  expect(f.respond).toHaveBeenCalledTimes(2); expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
});
it('does not show any prompt without a main-process request', async () => {
  const f = fixture(); render(<CloseConfirmationModal api={f.api} />);
  await waitFor(() => expect(f.read).toHaveBeenCalledOnce());
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument(); expect(f.respond).not.toHaveBeenCalled();
});
