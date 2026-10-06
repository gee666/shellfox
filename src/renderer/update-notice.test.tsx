// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import './terminal-test-mocks';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { failure, success } from '../shared/contracts';
import type { ManagerApi, UpdateStatusDto } from '../shared/contracts';
import { App } from './App';
import { mockApi } from './test-fixtures';

afterEach(cleanup);
beforeEach(() => localStorage.clear());
const status = (over: Partial<UpdateStatusDto> = {}): UpdateStatusDto => ({ current: '0.1.1', latest: '0.3.0', available: true, command: 'shellfox update', url: 'https://github.com/gee666/shellfox/releases', ...over });
function setup(result: Awaited<ReturnType<NonNullable<ManagerApi['getUpdateStatus']>>> | 'missing') {
  const fixture = mockApi(); const getUpdateStatus = vi.fn<NonNullable<ManagerApi['getUpdateStatus']>>(async () => result === 'missing' ? failure('INTERNAL', 'x') : result);
  const api: ManagerApi = result === 'missing' ? fixture.api : { ...fixture.api, getUpdateStatus };
  return { api, getUpdateStatus };
}
async function open(api: ManagerApi) {
  const user = userEvent.setup(); render(<App api={api} />);
  await screen.findByRole('button', { name: 'Select session Session 1' });
  return user;
}
describe('update notice', () => {
  it('shows a minimal notice with the command when a newer version exists', async () => {
    const { api, getUpdateStatus } = setup(success(status()));
    await open(api);
    const notice = await screen.findByRole('status');
    expect(notice).toHaveTextContent('Shellfox 0.3.0 is available — run shellfox update');
    expect(notice.querySelector('code')).toHaveTextContent('shellfox update');
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
});
