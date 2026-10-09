// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import './terminal-test-mocks';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { failure, success } from '../shared/contracts';
import type { ManagerApi, UpdateStatusDto } from '../shared/contracts';
import { App } from './App';
import { UpdateNotice, UPDATE_POLL_MS, DOWNLOAD_POLL_MS } from './UpdateNotice';
import { mockApi } from './test-fixtures';

afterEach(() => { cleanup(); vi.useRealTimers(); });
beforeEach(() => localStorage.clear());
const status = (over: Partial<UpdateStatusDto> = {}): UpdateStatusDto => ({ current: '0.3.4', latest: '0.3.5', available: true, supported: true, phase: 'idle', received: 0, total: null, error: null, command: 'shellfox update', url: 'https://github.com/gee666/shellfox/releases', ...over });
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
  it('shows the new version label and one round download icon', async () => {
    const { api, getUpdateStatus } = setup(success(status()));
    await open(api);
    const notice = await screen.findByRole('status');
    expect(notice.textContent).toBe('>.new version available');
    expect(notice.querySelectorAll('button')).toHaveLength(1);
    expect(screen.getByRole('button', { name: 'Download v0.3.5' })).toBeEnabled();
    expect(getUpdateStatus).toHaveBeenCalledTimes(1);
  });
  it('is hidden when up to date, on failure, and when the API lacks the method', async () => {
    for (const result of [success(status({ available: false })), success(status({ latest: null, available: false })), failure('INTERNAL', 'offline'), 'missing' as const]) {
      const { api, getUpdateStatus } = setup(result); await open(api);
      if (result !== 'missing') await waitFor(() => expect(getUpdateStatus).toHaveBeenCalled());
      expect(screen.queryByRole('status')).not.toBeInTheDocument(); cleanup();
    }
  });
  it('shows only progress and ETA, then a separate install action with explicit consent', async () => {
    let current = status();
    const f = mockApi();
    const downloadUpdate = vi.fn(async () => { current = status({ phase: 'downloading', received: 25, total: 100 }); return success(current); });
    const installUpdate = vi.fn(async () => success(status({ phase: 'ready' })));
    const api: ManagerApi = { ...f.api, getUpdateStatus: async () => success(current), downloadUpdate, installUpdate };
    const user = await open(api); await user.click(await screen.findByRole('button', { name: 'Download v0.3.5' }));
    expect(await screen.findByRole('progressbar', { name: 'Update download' })).toHaveAttribute('value', '25');
    expect(screen.getByRole('status').textContent).toBe('…');
    expect(screen.getByRole('status').querySelector('button')).toBeNull();
    expect(installUpdate).not.toHaveBeenCalled();
    current = status({ phase: 'ready', received: 100, total: 100 });
    const install = await screen.findByRole('button', { name: 'install and restart' });
    expect(screen.getByRole('status').textContent).toBe('install and restart');
    await user.click(install);
    expect(installUpdate).toHaveBeenCalledWith({ confirmCloseTerminals: true });
    expect(await screen.findByRole('button', { name: 'install and restart' })).toBeEnabled();
    expect(downloadUpdate).toHaveBeenCalledTimes(1);
  });
  it('estimates remaining time from progress and resets on stalls or retries', async () => {
    vi.useFakeTimers(); let current = status({ phase: 'downloading', received: 0, total: 100 });
    const { api } = setup(success(current));
    render(<UpdateNotice api={{ ...api, getUpdateStatus: async () => success(current) }} />);
    await act(async () => {});
    expect(screen.getByLabelText('Estimated time remaining')).toHaveTextContent('…');
    current = { ...current, received: 25 };
    await act(async () => { await vi.advanceTimersByTimeAsync(DOWNLOAD_POLL_MS); });
    expect(screen.getByLabelText('Estimated time remaining')).toHaveTextContent('2s');
    await act(async () => { await vi.advanceTimersByTimeAsync(DOWNLOAD_POLL_MS); });
    expect(screen.getByLabelText('Estimated time remaining')).toHaveTextContent('…');
    current = { ...current, received: 0 };
    await act(async () => { await vi.advanceTimersByTimeAsync(DOWNLOAD_POLL_MS); });
    expect(screen.getByLabelText('Estimated time remaining')).toHaveTextContent('…');
    current = { ...current, received: 1, total: 1000 };
    await act(async () => { await vi.advanceTimersByTimeAsync(DOWNLOAD_POLL_MS); });
    expect(screen.getByLabelText('Estimated time remaining')).toHaveTextContent('9m');
  });
  it('keeps unsupported details in the tooltip and preserves actionable errors', async () => {
    const { api } = setup(success(status({ supported: false, reason: 'Portable ZIP builds cannot self-update.' })));
    await open(api);
    const button = await screen.findByRole('button', { name: 'Download v0.3.5' });
    expect(button).toBeDisabled(); expect(button.title).toContain('Portable ZIP');
    expect(screen.getByRole('status').textContent).toBe('>.new version available'); cleanup();
    render(<UpdateNotice api={{ ...api, getUpdateStatus: async () => success(status({ phase: 'error', error: 'Checksum mismatch.' })) }} />);
    expect(await screen.findByRole('alert')).toHaveTextContent('Checksum mismatch.');
    expect(screen.getByRole('button', { name: 'Download v0.3.5' })).toBeEnabled();
  });
  it('keeps the install button disabled during handoff without extra text', async () => {
    const { api } = setup(success(status({ phase: 'installing' })));
    render(<UpdateNotice api={api} />);
    expect(await screen.findByRole('button', { name: 'install and restart' })).toBeDisabled();
    expect(screen.getByRole('status').textContent).toBe('install and restart');
  });
  it('checks periodically and shows a later release, then stops polling on unmount', async () => {
    vi.useFakeTimers(); let current = status({ available: false });
    const f = mockApi(), getUpdateStatus = vi.fn(async () => success(current));
    render(<UpdateNotice api={{ ...f.api, getUpdateStatus }} />);
    await act(async () => {});
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
    current = status({ latest: '0.4.0' });
    await act(async () => { await vi.advanceTimersByTimeAsync(UPDATE_POLL_MS); });
    expect(screen.getByRole('status').textContent).toBe('>.new version available');
    cleanup(); await vi.advanceTimersByTimeAsync(UPDATE_POLL_MS); expect(getUpdateStatus).toHaveBeenCalledTimes(2);
  });
});
