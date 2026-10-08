// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import './terminal-test-mocks';
import { terminalMocks } from './terminal-test-mocks';
import { StrictMode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { failure, success } from '../shared/contracts';
import type { TerminalEvent } from '../shared/contracts';
import { App } from './App';
import { deferred, mockApi, profiles, session, snapshot } from './test-fixtures';
import { SIDEBAR_WIDTH_KEY } from './sidebar-width';

afterEach(cleanup);
beforeEach(() => localStorage.clear());
async function ready(api = mockApi().api) {
  const user = userEvent.setup(); const rendered = render(<App api={api} />);
  await screen.findByRole('button', { name: 'Select session Session 1' });
  await waitFor(() => expect(screen.getByRole('button', { name: 'New session' })).toBeEnabled());
  return { user, ...rendered };
}

describe('compact workspace', () => {
  it('has only a minimal disconnected message without a preload', () => {
    render(<App />); expect(screen.getByText('Manager connection unavailable')).toBeVisible();
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });
  it('shows no heading, details, badges or nested terminal lists', async () => {
    await ready();
    expect(screen.queryByRole('heading')).not.toBeInTheDocument();
    expect(screen.getAllByRole('tab')).toHaveLength(1);
    expect(screen.getByRole('button', { name: 'Select session Session 1' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.queryByText(/agents|Local backend|Monitoring/)).not.toBeInTheDocument();
  });
  it('hides closed tabs, closes immediately without agents and activates the right then left neighbour', async () => {
    const item = session();
    item.tabs.push({ ...session(2).tabs[0]!, sessionId: item.id, title: 'Second' }, { ...session(3).tabs[0]!, sessionId: item.id, title: 'Third' }, { ...session(4).tabs[0]!, sessionId: item.id, title: 'Hidden', lifecycle: 'closed' });
    const fixture = mockApi(snapshot([item])); const { user } = await ready(fixture.api);
    expect(screen.queryByRole('tab', { name: /Hidden/ })).not.toBeInTheDocument();
    await user.click(screen.getByRole('tab', { name: /Second/ }));
    await user.click(screen.getByRole('button', { name: 'Close terminal Second' }));
    await waitFor(() => expect(screen.queryByRole('tab', { name: /Second/ })).not.toBeInTheDocument());
    expect(screen.getByRole('tab', { name: /Third/ })).toHaveAttribute('aria-selected', 'true');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Close terminal Third' }));
    await waitFor(() => expect(screen.getByRole('tab', { name: /Shell 1/ })).toHaveAttribute('aria-selected', 'true'));
    await user.click(screen.getByRole('button', { name: 'Close terminal Shell 1' }));
    expect(await screen.findByText('>. Shellfox')).toBeVisible();
    expect(screen.getByRole('button', { name: 'New terminal' })).toBeEnabled();
    expect(fixture.api.activateSession).not.toHaveBeenCalled();
  });
  it('does not let a delayed close steal a newer tab selection', async () => {
    const item = session();
    item.tabs.push({ ...session(2).tabs[0]!, sessionId: item.id, title: 'Second' }, { ...session(3).tabs[0]!, sessionId: item.id, title: 'Third' });
    const fixture = mockApi(snapshot([item]));
    const close = fixture.api.closeTab.getMockImplementation()!;
    const pending = deferred<Awaited<ReturnType<typeof fixture.api.closeTab>>>();
    fixture.api.closeTab.mockReturnValueOnce(pending.promise);
    const { user } = await ready(fixture.api);
    await user.click(screen.getByRole('button', { name: 'Close terminal Shell 1' }));
    await user.click(screen.getByRole('tab', { name: /Third/ }));
    await act(async () => pending.resolve(await close({ tabId: item.tabs[0]!.id, generation: item.tabs[0]!.generation })));
    await waitFor(() => expect(screen.queryByRole('tab', { name: /Shell 1/ })).not.toBeInTheDocument());
    expect(screen.getByRole('tab', { name: /Third/ })).toHaveAttribute('aria-selected', 'true');
  });

  it('keeps alternate-profile menu access when the saved default is unavailable', async () => {
    const initial = snapshot();
    const fixture = mockApi(initial);
    fixture.api.getTerminalProfiles.mockResolvedValue(success({
      profiles: [{ ...profiles[0]!, available: false, unavailableReason: 'Not installed' }, profiles[1]!],
      defaultProfileId: 'pwsh', lifetime: 'app-owned', shellSurvival: false,
    }));
    const user = userEvent.setup(); render(<App api={fixture.api} />);
    const plus = await screen.findByRole('button', { name: 'New terminal' });
    await waitFor(() => expect(plus).toBeEnabled());
    await user.click(plus); expect(fixture.api.addTab).toHaveBeenCalledWith({ sessionId: session().id });
    fireEvent.contextMenu(plus);
    expect(screen.getByRole('menuitem', { name: 'PowerShell 7' })).toBeDisabled();
    await user.click(screen.getByRole('menuitem', { name: 'Ubuntu' }));
    await waitFor(() => expect(fixture.api.addTab).toHaveBeenCalledWith({ sessionId: session().id, profileId: 'wsl:Ubuntu' }));
  });

  it('shows close-time settings failures as a toast and retains the draft on reopen', async () => {
    const fixture = mockApi();
    const pending = deferred<Awaited<ReturnType<typeof fixture.api.saveSettings>>>();
    fixture.api.saveSettings.mockReturnValueOnce(pending.promise);
    const { user } = await ready(fixture.api);
    await user.click(screen.getByRole('button', { name: 'Settings' }));
    await user.click(screen.getByRole('button', { name: 'Use #60a5fa' }));
    await user.keyboard('{Escape}');
    expect(fixture.api.saveSettings).toHaveBeenCalledTimes(1);
    await act(async () => pending.resolve(failure('STORAGE_FAILED', 'Could not save preferences.')));
    expect(await screen.findByRole('alert')).toHaveTextContent('Could not save preferences.');
    await user.click(screen.getByRole('button', { name: 'Dismiss message' }));
    await user.click(screen.getByRole('button', { name: 'Settings' }));
    expect(screen.getByRole('button', { name: 'Use #60a5fa' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('alert')).toHaveTextContent('Could not save preferences.');
    await user.keyboard('{Escape}');
    expect(fixture.api.saveSettings).toHaveBeenCalledTimes(1);
  });

  it('confirms only agent tabs and fences the generation originally shown', async () => {
    const item = session(); item.tabs[0]!.agents = 1;
    const fixture = mockApi(snapshot([item])); const { user } = await ready(fixture.api);
    await user.click(screen.getByRole('button', { name: 'Close terminal Shell 1' }));
    const dialog = screen.getByRole('dialog', { name: 'Close terminal Shell 1?' });
    expect(within(dialog).getByText('An agent is running in this tab. Close?')).toBeVisible();
    expect(fixture.api.closeTab).not.toHaveBeenCalled();
    const updated = session(); updated.tabs[0]!.generation = '00000000-0000-4000-b000-000000000002';
    act(() => fixture.emit({ ...snapshot([updated]), revision: 2 }));
    await user.click(within(dialog).getByRole('button', { name: 'Close' }));
    expect(fixture.api.closeTab).toHaveBeenCalledWith({ tabId: item.tabs[0]!.id, generation: item.tabs[0]!.generation });
    expect(fixture.api.settleSession).not.toHaveBeenCalled();
  });
  it('supports middle-click close and keyboard tab navigation', async () => {
    const item = session(); item.tabs.push({ ...session(2).tabs[0]!, sessionId: item.id, title: 'Second' });
    const fixture = mockApi(snapshot([item])); const { user } = await ready(fixture.api);
    screen.getByRole('tab', { name: /Shell 1/ }).focus(); await user.keyboard('{ArrowRight}');
    expect(screen.getByRole('tab', { name: /Second/ })).toHaveAttribute('aria-selected', 'true');
    fireEvent(screen.getByRole('tab', { name: /Second/ }), new MouseEvent('auxclick', { bubbles: true, button: 1 }));
    await waitFor(() => expect(screen.queryByRole('tab', { name: /Second/ })).not.toBeInTheDocument());
  });
  it('creates from the native chooser with basename and distinct request IDs', async () => {
    const fixture = mockApi(); const { user } = await ready(fixture.api);
    await user.click(screen.getByRole('button', { name: 'New session' }));
    await user.click(screen.getByRole('button', { name: 'Browse folders' }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Select session same folder' })).toHaveAttribute('aria-pressed', 'true'));
    await user.click(screen.getByRole('button', { name: 'New session' }));
    await user.click(screen.getByRole('button', { name: 'Browse folders' }));
    await waitFor(() => expect(fixture.api.createSession).toHaveBeenCalledTimes(2));
    const [first, second] = fixture.api.createSession.mock.calls.map(call => call[0]);
    expect(first).toMatchObject({ title: 'same folder', cwd: 'C:\\projects\\same folder' });
    expect(first!.requestId).not.toBe(second!.requestId);
    expect(fixture.api.chooseDirectory).toHaveBeenCalledTimes(2);
    expect(fixture.api.addTab).not.toHaveBeenCalled(); expect(fixture.api.activateSession).not.toHaveBeenCalled();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });
  it('keeps waiting for a folder picker left open longer than the IPC deadline', async () => {
    const fixture = mockApi(); const pick = deferred<Awaited<ReturnType<typeof fixture.api.chooseDirectory>>>();
    fixture.api.chooseDirectory.mockReturnValueOnce(pick.promise);
    const { user } = await ready(fixture.api);
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    try {
      fireEvent.click(screen.getByRole('button', { name: 'New session' }));
      fireEvent.click(screen.getByRole('button', { name: 'Browse folders' }));
      await act(async () => { vi.advanceTimersByTime(120_000); });
      expect(screen.queryByText(/did not confirm this request/)).not.toBeInTheDocument();
      await act(async () => { pick.resolve(success({ cwd: 'C:\\projects\\slow pick' })); });
    } finally { vi.useRealTimers(); }
    await waitFor(() => expect(fixture.api.createSession).toHaveBeenCalledWith(expect.objectContaining({ cwd: 'C:\\projects\\slow pick', title: 'slow pick' })));
    void user;
  });
  it('does nothing after chooser cancellation and ensures a terminal when creation returns no open tabs', async () => {
    const fixture = mockApi(); fixture.api.chooseDirectory.mockResolvedValueOnce(success(null));
    const create = fixture.api.createSession.getMockImplementation()!;
    fixture.api.createSession.mockImplementation(async input => {
      const result = await create(input);
      return result.ok ? success({ ...result.value, tabs: [] }) : result;
    });
    const { user } = await ready(fixture.api);
    await user.click(screen.getByRole('button', { name: 'New session' }));
    await user.click(screen.getByRole('button', { name: 'Browse folders' })); expect(fixture.api.createSession).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Browse folders' }));
    await waitFor(() => expect(fixture.api.addTab).toHaveBeenCalledWith({ sessionId: session(2).id }));
  });
  it('lets the backend choose the cwd default or sends an explicit profile from the plus context menu', async () => {
    const fixture = mockApi(); const { user } = await ready(fixture.api);
    await user.click(screen.getByRole('button', { name: 'New terminal' }));
    expect(await screen.findByRole('tab', { name: /Shell 2/ })).toHaveAttribute('aria-selected', 'true');
    expect(fixture.api.addTab).toHaveBeenLastCalledWith({ sessionId: session().id });
    fireEvent.contextMenu(screen.getByRole('button', { name: 'New terminal' }), { clientX: 300, clientY: 28 });
    await user.click(screen.getByRole('menuitem', { name: 'Ubuntu' }));
    await waitFor(() => expect(fixture.api.addTab).toHaveBeenLastCalledWith({ sessionId: session().id, profileId: 'wsl:Ubuntu' }));
  });
  it.each(['win32', 'linux'])('normal plus requests retain platform defaults on %s for a WSL cwd with an old PowerShell tab', async platform => {
    const item = session(1, { cwd: String.raw`\\wsl.localhost\Ubuntu\var\www` });
    const initial = snapshot([item]); initial.probe.platform = platform;
    initial.settings.terminalProfileId = 'wsl:Ubuntu';
    const fixture = mockApi(initial); const { user } = await ready(fixture.api);
    await user.click(screen.getByRole('button', { name: 'New terminal' }));
    await waitFor(() => expect(fixture.api.addTab).toHaveBeenCalledWith({ sessionId: item.id, ...(platform === 'linux' ? { profileId: 'wsl:Ubuntu' } : {}) }));
  });
  it('shows archived legacy sessions as read-only history without external terminals, Focus or Restore', async () => {
    const item = session(2, { adapterId: 'windows-terminal', terminalLifetime: 'external-legacy', settledAt: '2026-10-04T11:00:00.000Z', status: 'settled' });
    item.tabs[0]!.terminalKind = 'external-legacy'; item.tabs[0]!.title = '<img src=x onerror=alert(1)>';
    const fixture = mockApi(); fixture.api.getHistory.mockResolvedValue(success({ items: [item], total: 1, page: 1, pageSize: 20 }));
    const { user } = await ready(fixture.api);
    await user.click(await screen.findByRole('button', { name: 'Archived · 1' }));
    const row = screen.getByRole('button', { name: 'Select session Session 2' }); await user.click(row);
    expect(screen.getByText('Archived session')).toBeVisible(); expect(screen.queryByLabelText('Interactive terminal')).not.toBeInTheDocument();
    expect(screen.queryByRole('tab')).not.toBeInTheDocument(); expect(document.querySelector('img')).toBeNull();
    expect(screen.queryByText('Runs in an external terminal')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Focus' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Restore' })).not.toBeInTheDocument();
    fireEvent.contextMenu(row); expect(screen.queryByRole('menuitem', { name: 'Restore' })).not.toBeInTheDocument();
    expect(fixture.api.focusSession).not.toHaveBeenCalled(); expect(fixture.api.unsettleSession).not.toHaveBeenCalled();
  });
  it('filters obsolete external tabs from tasks that have been explicitly converted to embedded terminals', async () => {
    const item = session(), external = { ...item.tabs[0]!, id: session(2).tabs[0]!.id, title: 'Old external tab', terminalKind: 'external-legacy' as const };
    item.tabs.unshift(external);
    const fixture = mockApi(snapshot([item])); await ready(fixture.api);
    expect(screen.queryByRole('tab', { name: /Old external tab/ })).not.toBeInTheDocument();
    expect(screen.getByRole('tab', { name: /Shell 1/ })).toBeVisible();
    await waitFor(() => expect(fixture.api.attachTerminal).toHaveBeenCalledWith(expect.objectContaining({ tabId: item.tabs[1]!.id })));
    expect(fixture.api.focusSession).not.toHaveBeenCalled();
  });
  it('leaves the terminal mounted and receiving output while settings is open', async () => {
    const fixture = mockApi(); const { user } = await ready(fixture.api);
    await waitFor(() => expect(fixture.api.attachTerminal).toHaveBeenCalled());
    const original = terminalMocks.terminals.at(-1);
    await user.click(screen.getByRole('button', { name: 'Settings' }));
    act(() => fixture.emitTerminal({ type: 'data', tabId: session().tabs[0]!.id, generation: session().tabs[0]!.generation, sequence: 1, data: 'live output\r\n' }));
    expect(original.output).toContain('live output'); expect(original.disposed).toBe(false);
    expect(screen.getByLabelText('Interactive terminal')).toBeInTheDocument();
    await user.keyboard('{Escape}'); expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    await waitFor(() => expect(original.focus).toHaveBeenCalled());
  });
  it('renames inline and archives with a small backend-required confirmation', async () => {
    const fixture = mockApi(); fixture.api.settleSession.mockResolvedValueOnce(failure('SETTLE_CONFIRM_REQUIRED', 'Still running'));
    const { user } = await ready(fixture.api);
    fireEvent.contextMenu(screen.getByRole('button', { name: 'Select session Session 1' }));
    await user.click(screen.getByRole('menuitem', { name: 'Rename' }));
    const input = screen.getByLabelText('Session title'); await user.clear(input); await user.type(input, 'A task{Enter}');
    const row = await screen.findByRole('button', { name: 'Select session A task' });
    fireEvent.contextMenu(row); await user.click(screen.getByRole('menuitem', { name: 'Archive' }));
    const confirm = await screen.findByRole('dialog', { name: 'Archive session' });
    expect(within(confirm).getByRole('button', { name: 'Archive' })).toHaveFocus();
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('dialog', { name: 'Archive session' })).not.toBeInTheDocument();
    fixture.api.settleSession.mockResolvedValueOnce(failure('SETTLE_CONFIRM_REQUIRED', 'Still running'));
    fireEvent.contextMenu(row); await user.click(screen.getByRole('menuitem', { name: 'Archive' }));
    const reopened = await screen.findByRole('dialog', { name: 'Archive session' });
    await user.click(within(reopened).getByRole('button', { name: 'Archive' }));
    await waitFor(() => expect(fixture.api.settleSession).toHaveBeenLastCalledWith({ sessionId: session().id, confirmActive: true }));
    expect(fixture.api.closeTab).not.toHaveBeenCalled(); expect(await screen.findByText('Archived session')).toBeVisible();
  });
  it('keeps archived sessions collapsed and restores from the empty read-only view', async () => {
    const historical = session(2, { settledAt: '2026-10-04T11:00:00.000Z', status: 'settled' });
    const fixture = mockApi(); fixture.api.getHistory.mockResolvedValue(success({ items: [historical], total: 1, page: 1, pageSize: 20 }));
    const { user } = await ready(fixture.api);
    expect(screen.queryByRole('button', { name: 'Select session Session 2' })).not.toBeInTheDocument();
    await user.click(await screen.findByRole('button', { name: 'Archived · 1' }));
    await user.click(screen.getByRole('button', { name: 'Select session Session 2' }));
    expect(screen.getByText('Archived session')).toBeVisible(); expect(screen.queryByLabelText('Interactive terminal')).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Restore' })); expect(fixture.api.unsettleSession).toHaveBeenCalledWith({ sessionId: historical.id });
  });
  it('restores, clamps, persists and resets sidebar width', async () => {
    localStorage.setItem(SIDEBAR_WIDTH_KEY, '280'); const fixture = mockApi(); const { user } = await ready(fixture.api);
    const handle = screen.getByRole('separator', { name: 'Sidebar width' });
    expect(handle).toHaveAttribute('aria-valuenow', '280'); handle.focus(); await user.keyboard('{ArrowRight}');
    expect(localStorage.getItem(SIDEBAR_WIDTH_KEY)).toBe('284');
    fireEvent.doubleClick(handle); expect(localStorage.getItem(SIDEBAR_WIDTH_KEY)).toBe('220');
    Object.defineProperty(window, 'innerWidth', { configurable: true, value: 360 });
    fireEvent(window, new Event('resize')); expect(handle).toHaveAttribute('aria-valuenow', '180');
    Object.defineProperty(window, 'innerWidth', { configurable: true, value: 1024 });
  });
  it('drags the sidebar with pointer capture and persists the final width', async () => {
    const { unmount } = await ready();
    const handle = screen.getByRole('separator', { name: 'Sidebar width' });
    let captured = false;
    Object.assign(handle, {
      setPointerCapture: vi.fn(() => { captured = true; }),
      hasPointerCapture: vi.fn(() => captured),
      releasePointerCapture: vi.fn(() => { captured = false; }),
    });
    const pointer = (type: string, x: number) => {
      const event = new MouseEvent(type, { bubbles: true, clientX: x, button: 0 });
      Object.defineProperty(event, 'pointerId', { value: 1 });
      fireEvent(handle, event);
    };
    pointer('pointerdown', 220); pointer('pointermove', 320); pointer('pointerup', 320);
    expect(handle).toHaveAttribute('aria-valuenow', '320');
    expect(localStorage.getItem(SIDEBAR_WIDTH_KEY)).toBe('320');
    expect(handle.releasePointerCapture).toHaveBeenCalledWith(1);
    unmount();
    render(<App api={mockApi().api} />);
    expect(screen.getByRole('separator')).toHaveAttribute('aria-valuenow', '320');
  });

  it('maps activity to pulsing green only for agent-running tabs', async () => {
    const item = session(); item.tabs[0]!.status = 'running';
    const fixture = mockApi(snapshot([item])); await ready(fixture.api);
    expect(screen.getAllByLabelText('Agent idle')).toHaveLength(2);
    act(() => fixture.emitTerminal({ type: 'activity', tabId: item.tabs[0]!.id, busy: true } as TerminalEvent));
    expect(screen.getAllByLabelText('Agent working').every(dot => dot.classList.contains('dot-busy'))).toBe(true);
    item.tabs[0]!.status = 'waiting';
    act(() => fixture.emit({ ...snapshot([item]), revision: 2 }));
    await waitFor(() => expect(screen.getAllByLabelText('Shell')).toHaveLength(2));
  });
  it('shows errors as dismissable toasts and never repeats a failed mutation', async () => {
    const fixture = mockApi(); fixture.api.addTab.mockRejectedValueOnce(new Error('secret')); const { user } = await ready(fixture.api);
    await user.click(screen.getByRole('button', { name: 'New terminal' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('No automatic retry'); expect(fixture.api.addTab).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole('button', { name: 'Dismiss message' })); expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });
  it('cleans up both metadata activity and output subscriptions under StrictMode', async () => {
    const fixture = mockApi(); const rendered = render(<StrictMode><App api={fixture.api} /></StrictMode>);
    await screen.findByRole('tab', { name: /Shell 1/ }); rendered.unmount();
    await waitFor(() => expect(fixture.terminalUnsubscribe).toHaveBeenCalledTimes(fixture.api.subscribeTerminal.mock.calls.length));
    expect(terminalMocks.terminals.every(term => term.disposed)).toBe(true);
  });
});
