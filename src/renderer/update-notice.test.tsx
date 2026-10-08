// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import './terminal-test-mocks';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { failure, success } from '../shared/contracts';
import type { ManagerApi, UpdateStatusDto } from '../shared/contracts';
import { App } from './App';
import { UpdateNotice, UPDATE_POLL_MS } from './UpdateNotice';
import { mockApi } from './test-fixtures';

afterEach(() => { cleanup(); vi.useRealTimers(); });
beforeEach(() => localStorage.clear());
const status = (over: Partial<UpdateStatusDto> = {}): UpdateStatusDto => ({ current: '0.1.1', latest: '0.3.0', available: true, supported: true, phase: 'idle', received: 0, total: null, error: null, command: 'shellfox update', url: 'https://github.com/gee666/shellfox/releases', ...over });
function setup(result: Awaited<ReturnType<NonNullable<ManagerApi['getUpdateStatus']>>> | 'missing') {
  const fixture = mockApi(); const getUpdateStatus = vi.fn<NonNullable<ManagerApi['getUpdateStatus']>>(async () => result === 'missing' ? failure('INTERNAL', 'x') : result);
  const api: ManagerApi = result === 'missing' ? fixture.api : { ...fixture.api, getUpdateStatus, downloadUpdate: vi.fn(async () => success(status({ phase: 'downloading', received: 5, total: 10 }))), installUpdate: vi.fn(async () => success(status({ phase: 'ready' }))) };
  return { api, getUpdateStatus };
}
async function open(api: ManagerApi) {
  const user = userEvent.setup(); render(<App api={api} />);
  await screen.findByRole('button', { name: 'Select session Session 1' });
  return user;
}
describe('update notice', () => {
  it('shows an in-app download button instead of a CLI command', async () => {
    const { api, getUpdateStatus } = setup(success(status()));
    await open(api);
    const notice = await screen.findByRole('status');
    expect(notice).toHaveTextContent('Shellfox 0.3.0 is available');
    expect(notice).not.toHaveTextContent('shellfox update');
    expect(screen.getByRole('button', { name: 'Download update' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Hide update notice' })).toBeVisible();
    expect(getUpdateStatus).toHaveBeenCalledTimes(1);
  });
  it('is hidden when up to date, on failure, and when the API lacks the method', async () => {
    for (const result of [success(status({ latest: '0.1.1', available: false })), success(status({ latest: null, available: false })), failure('INTERNAL', 'offline'), 'missing' as const]) {
      const { api, getUpdateStatus } = setup(result); await open(api);
      if (result !== 'missing') await waitFor(() => expect(getUpdateStatus).toHaveBeenCalled());
      expect(screen.queryByText(/is available/)).not.toBeInTheDocument(); expect(screen.queryByRole('button', { name: 'Hide update notice' })).not.toBeInTheDocument();
      cleanup();
    }
  });
  it('hides on dismiss without persisting, and returns on the next start', async () => {
    const { api } = setup(success(status()));
    const user = await open(api);
    const button = await screen.findByRole('button', { name: 'Hide update notice' }); const before = JSON.stringify({ ...localStorage });
    await user.click(button);
    expect(screen.queryByText(/is available/)).not.toBeInTheDocument();
    expect(JSON.stringify({ ...localStorage })).toBe(before); expect(sessionStorage.length).toBe(0);
    cleanup();
    await open(api);
    expect(await screen.findByText(/is available/)).toBeVisible();
  });
  it('reports progress, then requires a separate install action with explicit terminal-close consent', async () => {
    let current = status();
    const f = mockApi();
    const downloadUpdate = vi.fn(async () => { current = status({ phase: 'downloading', received: 25, total: 100 }); return success(current); });
    const installUpdate = vi.fn(async () => success(status({ phase: 'ready' })));
    const api: ManagerApi = { ...f.api, getUpdateStatus: async () => success(current), downloadUpdate, installUpdate };
    const user = await open(api); await user.click(await screen.findByRole('button', { name: 'Download update' }));
    expect(await screen.findByRole('progressbar', { name: 'Update download' })).toHaveAttribute('value', '25');
    expect(screen.getByText(/25%/)).toBeVisible(); expect(installUpdate).not.toHaveBeenCalled();
    expect(screen.queryByRole('button', { name: 'Hide update notice' })).not.toBeInTheDocument();
    current = status({ phase: 'ready', received: 100, total: 100 });
    const install = await screen.findByRole('button', { name: 'Install and restart' });
    expect(screen.getByText(/Installing closes all Shellfox terminals/)).toBeVisible();
    await user.click(install);
    expect(installUpdate).toHaveBeenCalledWith({ confirmCloseTerminals: true });
    // Native Cancel returns ready. No additional download and no automatic retry.
    expect(await screen.findByRole('button', { name: 'Install and restart' })).toBeEnabled();
    expect(downloadUpdate).toHaveBeenCalledTimes(1);
  });
  it('keeps unsupported copies manual and reports failed downloads with a retry button', async () => {
    const { api } = setup(success(status({ supported: false, reason: 'Portable ZIP builds cannot self-update.' })));
    await open(api);
    expect(await screen.findByRole('button', { name: 'Download update' })).toBeDisabled();
    expect(screen.getByText(/Portable ZIP/)).toBeVisible(); cleanup();
    render(<UpdateNotice api={{ ...api, getUpdateStatus: async () => success(status({ phase: 'error', error: 'Checksum mismatch.' })) }} />);
    expect(await screen.findByRole('alert')).toHaveTextContent('Checksum mismatch.');
    expect(screen.getByRole('button', { name: 'Retry download' })).toBeEnabled();
  });
  it('checks periodically even after dismissal and shows a later release', async () => {
    vi.useFakeTimers(); let current = status();
    const f = mockApi(), getUpdateStatus = vi.fn(async () => success(current));
    render(<UpdateNotice api={{ ...f.api, getUpdateStatus }} />);
    await act(async () => {});
    act(() => screen.getByRole('button', { name: 'Hide update notice' }).click());
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
    await act(async () => { await vi.advanceTimersByTimeAsync(UPDATE_POLL_MS); });
    expect(getUpdateStatus).toHaveBeenCalledTimes(2); expect(screen.queryByRole('status')).not.toBeInTheDocument();
    current = status({ latest: '0.4.0' });
    await act(async () => { await vi.advanceTimersByTimeAsync(UPDATE_POLL_MS); });
    expect(screen.getByRole('status')).toHaveTextContent('Shellfox 0.4.0 is available');
    cleanup(); await vi.advanceTimersByTimeAsync(UPDATE_POLL_MS); expect(getUpdateStatus).toHaveBeenCalledTimes(3);
  });
});
