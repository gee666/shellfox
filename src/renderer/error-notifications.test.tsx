// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import './terminal-test-mocks';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { App } from './App';
import { Notifications, useNotify } from './components';
import { TerminalViewport } from './TerminalViewport';
import { TerminalRegistry } from './terminal-client';
import { terminalMocks } from './terminal-test-mocks';
import { failure } from '../shared/contracts';
import { deferred, mockApi, session, snapshot } from './test-fixtures';

const registries: TerminalRegistry[] = [];
afterEach(() => { cleanup(); registries.splice(0).forEach(registry => registry.dispose()); vi.useRealTimers(); });

function Notices() {
  const notify = useNotify();
  return <><button onClick={() => notify({ code: 'INTERNAL', message: 'Write failed', retryable: false })}>Fail</button>
    <button onClick={() => notify({ code: 'STORAGE_FAILED', message: 'Save failed', retryable: true })}>Save</button>
    <button onClick={() => notify({ message: 'Write failed', tone: 'info' })}>Info</button></>;
}

async function readyApp(fixture: ReturnType<typeof mockApi>) {
  render(<App api={fixture.api} />);
  await screen.findByRole('button', { name: 'Select session Session 1' });
  await waitFor(() => expect(fixture.api.attachTerminal).toHaveBeenCalled());
  await act(async () => {});
}

function viewport(visible = true) {
  const fixture = mockApi();
  const registry = new TerminalRegistry(fixture.api); registries.push(registry); registry.start();
  const view = render(<Notifications><TerminalViewport registry={registry} tabId={session().tabs[0]!.id} visible={visible} /></Notifications>);
  return { ...fixture, registry, ...view };
}

describe('error notification routing', () => {
  it('deduplicates repeats without extending their lifetime or undoing dismissal', () => {
    vi.useFakeTimers();
    render(<Notifications><Notices /></Notifications>);
    fireEvent.click(screen.getByText('Fail')); fireEvent.click(screen.getByText('Fail'));
    expect(screen.getAllByRole('alert')).toHaveLength(1);
    act(() => vi.advanceTimersByTime(5000)); fireEvent.click(screen.getByText('Fail'));
    act(() => vi.advanceTimersByTime(1000));
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    fireEvent.click(screen.getByText('Fail'));
    expect(screen.getByRole('alert')).toHaveTextContent('Write failed');
    fireEvent.click(screen.getByLabelText('Dismiss message')); fireEvent.click(screen.getByText('Fail'));
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    fireEvent.click(screen.getByText('Save')); fireEvent.click(screen.getByText('Info'));
    expect(screen.getByRole('alert')).toHaveTextContent('Save failed');
    expect(screen.getByRole('status')).toHaveTextContent('Write failed');
  });

  it('suppresses automatic profile, archive-count and usable-snapshot refresh failures', async () => {
    const fixture = mockApi();
    fixture.api.getTerminalProfiles.mockResolvedValue(failure('INTERNAL', 'Profile scan failed'));
    fixture.api.getHistory.mockResolvedValue(failure('STORAGE_FAILED', 'Archive count failed'));
    await readyApp(fixture);
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    fixture.api.getSnapshot.mockResolvedValueOnce(failure('INTERNAL', 'Refresh failed'));
    act(() => fixture.emit({ ...snapshot(), revision: 2 }));
    await waitFor(() => expect(fixture.api.getSnapshot).toHaveBeenCalledTimes(2));
    await act(async () => {});
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(screen.getByRole('tab', { name: /Shell 1/ })).toBeInTheDocument();
    // The same storage failure must be shown for an explicit history request.
    fireEvent.click(screen.getByRole('button', { name: 'Archived · 0' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Archive count failed');
  });

  it('still reports an initial manager connection failure', async () => {
    const fixture = mockApi();
    fixture.api.getSnapshot.mockResolvedValue(failure('INTERNAL', 'Manager unavailable'));
    render(<App api={fixture.api} />);
    expect(await screen.findByRole('alert')).toHaveTextContent('Manager unavailable');
  });

  it('keeps failed user mutations visible and never retries them automatically', async () => {
    const fixture = mockApi(); await readyApp(fixture);
    fixture.api.addTab.mockResolvedValueOnce(failure('DEPENDENCY_MISSING', 'Install the selected shell'));
    fireEvent.click(screen.getByRole('button', { name: 'New terminal' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Install the selected shell');
    expect(fixture.api.addTab).toHaveBeenCalledTimes(1);
  });

  it('shows a genuine attach failure locally with reconnect, not as a bottom-right toast', async () => {
    const fixture = viewport();
    await act(async () => {});
    fixture.api.attachTerminal.mockResolvedValueOnce(failure('INTERNAL', 'Terminal attach failed'));
    act(() => fixture.registry.refresh(session().tabs[0]!.id));
    expect(await screen.findByRole('alert')).toHaveTextContent('Terminal attach failed');
    expect(document.querySelector('.toasts .toast')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Reconnect terminal' }));
    await waitFor(() => expect(screen.queryByRole('alert')).not.toBeInTheDocument());
    expect(screen.queryByRole('button', { name: 'Reconnect terminal' })).not.toBeInTheDocument();
    expect(fixture.api.closeTab).not.toHaveBeenCalled();
  });

  it('does not toast transient delivery errors that recover successfully', async () => {
    const fixture = viewport(); await act(async () => {});
    const tab = session().tabs[0]!;
    act(() => fixture.emitTerminal({ type: 'error', tabId: tab.id, generation: tab.generation, error: { code: 'INTERNAL', message: 'Display credit expired', retryable: true } }));
    await waitFor(() => expect(fixture.api.attachTerminal).toHaveBeenCalledTimes(2));
    await act(async () => {});
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('keeps clipboard failures visible for the active terminal', async () => {
    const fixture = viewport(); await act(async () => {});
    fixture.api.readClipboardText.mockResolvedValueOnce(failure('INTERNAL', 'Clipboard unavailable'));
    act(() => terminalMocks.terminals.at(-1).keyHandler(new KeyboardEvent('keydown', { code: 'KeyV', ctrlKey: true, shiftKey: true })));
    expect(await screen.findByRole('alert')).toHaveTextContent('Clipboard unavailable');
    expect(document.querySelector('.toasts .toast')).not.toBeNull();
  });

  it('does not replay a consumed operation error on a later viewport mount', async () => {
    const fixture = viewport(); await act(async () => {});
    fixture.api.readClipboardText.mockResolvedValueOnce(failure('INTERNAL', 'Clipboard unavailable'));
    act(() => terminalMocks.terminals.at(-1).keyHandler(new KeyboardEvent('keydown', { code: 'KeyV', ctrlKey: true, shiftKey: true })));
    await screen.findByRole('alert');
    expect(fixture.registry.getState(session().tabs[0]!.id).operationError).toBeNull();
    fixture.unmount();
    render(<Notifications><TerminalViewport registry={fixture.registry} tabId={session().tabs[0]!.id} visible /></Notifications>);
    await act(async () => {});
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });
  it('ignores a late failed attach after the viewport unmounts', async () => {
    const fixture = viewport(); await act(async () => {});
    const pending = deferred<Awaited<ReturnType<typeof fixture.api.attachTerminal>>>();
    fixture.api.attachTerminal.mockReturnValueOnce(pending.promise);
    act(() => fixture.registry.refresh(session().tabs[0]!.id));
    await act(async () => {}); fixture.unmount();
    await act(async () => pending.resolve(failure('INTERNAL', 'Old attach failed')));
    expect(fixture.registry.getState(session().tabs[0]!.id).error).toBeNull();
    expect(fixture.registry.getState(session().tabs[0]!.id).operationError).toBeNull();
  });
});
