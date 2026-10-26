// @vitest-environment jsdom
import React, { type ReactNode } from 'react';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { failure, success, type SessionDto } from '../../src/shared/contracts';
import { profiles, session, snapshot } from '../../src/renderer/test-fixtures';
import { RemoteClient } from './client/client';
import { RemoteClientError } from './client';
import { TestTransport } from './client/test-fixtures';
import type { MachineView, ShellfoxModel } from './useShellfox';
import type { MenuConfig } from './ActionMenu';
import type { TerminalBridgeState } from './terminal/bridge';
import { TerminalOperationError } from './terminal/errors';
import { SessionScreen } from './SessionScreen';

type NativeProps = {
  children?: ReactNode; accessibilityLabel?: string; accessibilityRole?: string; disabled?: boolean;
  onPress?: () => void; onLongPress?: () => void; testID?: string;
};
type TerminalProps = { tabId: string; onError: (error: Error) => void; onState: (state: TerminalBridgeState) => void };
let terminalProps: TerminalProps | null = null;
vi.mock('react-native', () => ({
  Alert: { alert: vi.fn() },
  Text: ({ children, accessibilityLabel, testID }: NativeProps) => <span aria-label={accessibilityLabel} data-testid={testID}>{children}</span>,
  View: ({ children }: NativeProps) => <div>{children}</div>,
  ScrollView: ({ children }: NativeProps) => <div>{children}</div>,
  Pressable: ({ children, accessibilityLabel, accessibilityRole, disabled, onPress, onLongPress }: NativeProps) => <button role={accessibilityRole} aria-label={accessibilityLabel} disabled={disabled} onClick={onPress} onContextMenu={onLongPress}>{children}</button>,
}));
vi.mock('./ui', () => ({
  useUi: () => ({ colors: {}, styles: {} }),
  Button: ({ title, onPress }: { title: string; onPress: () => void }) => <button onClick={onPress}>{title}</button>,
  Note: ({ children }: NativeProps) => <span>{children}</span>,
}));
vi.mock('./terminal/TerminalView', () => ({ TerminalView: (props: TerminalProps) => { terminalProps = props; return <div data-testid="terminal" data-tab={props.tabId} />; } }));
vi.mock('./ActionMenu', () => ({ ActionMenu: ({ menu, close }: { menu: MenuConfig; close: () => void }) => (
  <div role="dialog">{menu.actions.map(action => <button key={action.title} onClick={() => { close(); action.action(); }}>{action.title}</button>)}</div>
) }));
vi.mock('./StatusIndicator', () => ({ StatusIndicator: ({ kind, label }: { kind: string; label: string }) => <span role="img" aria-label={label} data-kind={kind} /> }));
beforeEach(() => { vi.clearAllMocks(); terminalProps = null; });
afterEach(cleanup);
function setup(connected = true, item: SessionDto = session()) {
  item.tabs[0].status = 'running';
  const transport = new TestTransport();
  const client = new RemoteClient('host', transport, { createClientId: () => '10000000-0000-4000-8000-000000000001' });
  const request = vi.spyOn(client, 'request').mockResolvedValue(success({ configured: true }));
  let machine: MachineView = {
    peer: { id: 'host', hostname: 'workstation', online: true, addresses: ['100.64.0.2'] },
    client, state: { ...client.getState(), status: connected ? 'connected' : 'disconnected', connectionId: 'test' },
    snapshot: snapshot([item]), snapshotConnectionId: 'test', busyTabIds: { [item.tabs[0].id]: true },
  };
  let model: ShellfoxModel = {
    ready: true, loading: false, network: { status: 'online', loginUrl: null, error: null }, peers: [], machines: [machine],
    preferences: { hostname: 'phone', fontSize: 14, remembered: [] }, error: null,
    setError: vi.fn(), report: vi.fn(), updatePreferences: vi.fn(), connect: vi.fn(), disconnect: vi.fn(),
    login: vi.fn(), logout: vi.fn(), renameDevice: vi.fn(), refresh: vi.fn(), retryStart: vi.fn(),
  };
  const ui = () => <SessionScreen machine={machine} sessionId={item.id} model={model} back={vi.fn()} prompt={vi.fn()} />;
  const view = render(ui());
  const update = (changes: Partial<MachineView>) => {
    machine = { ...machine, ...changes }; model = { ...model, machines: [machine] }; view.rerender(ui());
  };
  const replace = (next: SessionDto) => update({ snapshot: snapshot([next]) });
  return { item, client, request, model, machine, view, update, replace };
}
async function press(name: string) { await act(async () => { fireEvent.click(screen.getByRole('button', { name })); }); }
function newLiveSession(item: SessionDto) {
  const created = { ...session(2).tabs[0], sessionId: item.id, title: 'New live shell', ordinal: 1 };
  const closed = { ...session(3).tabs[0], sessionId: item.id, lifecycle: 'closed' as const, title: 'Closed shell', ordinal: 99 };
  return { ...item, tabs: [created, ...item.tabs, closed] };
}
describe('compact terminal chrome', () => {
  it('keeps the session and host context with an accessible plus control and shared busy status', () => {
    const { item } = setup();
    expect(screen.getByText(item.title)).toBeInTheDocument();
    expect(screen.getByText('workstation')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'New terminal' })).toHaveTextContent('+');
    expect(screen.getByRole('img', { name: 'Agent working' })).toHaveAttribute('data-kind', 'busy');
    expect(screen.queryByText('+ Tab')).not.toBeInTheDocument();
    expect(screen.getByTestId('terminal')).toHaveAttribute('data-tab', item.tabs[0].id);
  });
  it('does not mount a terminal or enable creation while disconnected', () => {
    setup(false);
    expect(screen.getByRole('button', { name: 'New terminal' })).toBeDisabled();
    expect(screen.queryByTestId('terminal')).not.toBeInTheDocument();
  });
  it('adds the host default on a short press without requesting profiles, selecting the new live ID regardless of order', async () => {
    const f = setup(); const added = newLiveSession(f.item);
    f.request.mockResolvedValueOnce(success(added));
    vi.mocked(f.model.refresh).mockImplementation(async () => { f.replace(added); });
    await press('New terminal');
    expect(f.request).toHaveBeenCalledWith('addTab', { sessionId: f.item.id });
    expect(f.request).not.toHaveBeenCalledWith('getTerminalProfiles', expect.anything());
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(screen.getByTestId('terminal')).toHaveAttribute('data-tab', added.tabs[0].id);
  });
  it('opens the profile chooser only on long press and filters unavailable or elevated profiles', async () => {
    const f = setup();
    f.request.mockResolvedValueOnce(success({ profiles: [...profiles,
      { ...profiles[0], id: 'admin', label: 'Administrator', elevation: true },
      { ...profiles[0], id: 'missing', label: 'Unavailable', available: false },
    ], defaultProfileId: 'pwsh', lifetime: 'app-owned', shellSurvival: false }));
    await act(async () => { fireEvent.contextMenu(screen.getByRole('button', { name: 'New terminal' })); });
    expect(f.request).toHaveBeenCalledWith('getTerminalProfiles', {});
    expect(f.request).not.toHaveBeenCalledWith('addTab', expect.anything());
    expect(screen.getByRole('button', { name: 'PowerShell 7' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Administrator' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Unavailable' })).toBeNull();
    f.request.mockResolvedValueOnce(success(newLiveSession(f.item)));
    await press('Ubuntu');
    expect(f.request).toHaveBeenCalledWith('addTab', { sessionId: f.item.id, profileId: 'wsl:Ubuntu' });
  });
  it('still shows a chooser for one available profile rather than creating on long press', async () => {
    const f = setup();
    f.request.mockResolvedValueOnce(success({ profiles: [profiles[0]], defaultProfileId: 'pwsh', lifetime: 'app-owned', shellSurvival: false }));
    await act(async () => { fireEvent.contextMenu(screen.getByRole('button', { name: 'New terminal' })); });
    expect(screen.getByRole('button', { name: 'PowerShell 7' })).toBeInTheDocument();
    expect(f.request).not.toHaveBeenCalledWith('addTab', expect.anything());
  });
  it('preserves typed operation failures from the default add action', async () => {
    const f = setup(); f.request.mockResolvedValueOnce(failure('TARGET_LOST', 'Host disconnected.', true));
    await press('New terminal');
    const error = vi.mocked(f.model.report).mock.calls[0][0];
    expect(error).toBeInstanceOf(RemoteClientError);
    expect(error).toMatchObject({ detail: { code: 'TARGET_LOST', retryable: true } });
  });
  it('never mounts closed tabs and opens the new default shell without a stale global warning', async () => {
    const item = session(); item.tabs[0].lifecycle = 'closed';
    const f = setup(true, item);
    expect(screen.queryByTestId('terminal')).toBeNull(); expect(f.model.report).not.toHaveBeenCalled();
    expect(screen.queryByText('No open terminal.')).toBeNull();
    const added = newLiveSession(item); f.request.mockResolvedValueOnce(success(added));
    vi.mocked(f.model.refresh).mockImplementation(async () => { f.replace(added); });
    await press('New terminal');
    expect(screen.getByTestId('terminal')).toHaveAttribute('data-tab', added.tabs[0].id);
    expect(screen.queryByText(/terminal is closed/i)).toBeNull(); expect(f.model.report).not.toHaveBeenCalled();
  });
  it('keeps terminal errors local, clears them when a default tab opens, and ignores late callbacks from the previous tab', async () => {
    const f = setup(); const old = terminalProps!;
    act(() => old.onError(new Error('Old attach warning.')));
    expect(screen.getByText('Old attach warning.')).toBeInTheDocument(); expect(f.model.report).not.toHaveBeenCalled();
    const added = newLiveSession(f.item); f.request.mockResolvedValueOnce(success(added));
    vi.mocked(f.model.refresh).mockImplementation(async () => { f.replace(added); });
    await press('New terminal');
    expect(screen.queryByText('Old attach warning.')).toBeNull();
    act(() => terminalProps!.onError(new Error('Current warning.')));
    act(() => { old.onError(new Error('Late old warning.')); old.onState('attached'); });
    expect(screen.queryByText('Late old warning.')).toBeNull();
    expect(screen.getByText('Current warning.')).toBeInTheDocument(); expect(f.model.report).not.toHaveBeenCalled();
  });
  it('clears local warnings on generation or connection identity changes and attached state', () => {
    const f = setup(); act(() => terminalProps!.onError(new Error('Generation warning.')));
    f.replace({ ...f.item, tabs: [{ ...f.item.tabs[0], generation: session(8).tabs[0].generation }] });
    expect(screen.queryByText('Generation warning.')).toBeNull();
    act(() => terminalProps!.onError(new Error('Connection warning.')));
    f.update({ state: { ...f.machine.state, connectionId: 'next' } });
    expect(screen.queryByText('Connection warning.')).toBeNull();
    act(() => terminalProps!.onError(new Error('Temporary warning.')));
    act(() => terminalProps!.onState('attached'));
    expect(screen.queryByText('Temporary warning.')).toBeNull();
  });
  it('renders connection status live through attachment, disconnect, and reconnect without a dismiss notice', () => {
    const f = setup(); expect(screen.getByLabelText('Connection connecting...')).toBeInTheDocument();
    act(() => terminalProps!.onState('attached'));
    expect(screen.getByLabelText('Connection connected')).toBeInTheDocument();
    act(() => terminalProps!.onState('reconnecting'));
    expect(screen.getByLabelText('Connection reconnecting...')).toBeInTheDocument();
    f.update({ state: { ...f.machine.state, status: 'disconnected' } });
    expect(screen.getByLabelText('Connection disconnected')).toBeInTheDocument();
    f.update({ state: { ...f.machine.state, status: 'reconnecting', connectionId: null } });
    expect(screen.getByLabelText('Connection reconnecting...')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Dismiss' })).toBeNull(); expect(f.model.report).not.toHaveBeenCalled();
  });
  it.each(['NOT_FOUND', 'TARGET_LOST'] as const)('refreshes stale terminal identity on %s without a local or global notice', code => {
    const f = setup();
    act(() => terminalProps!.onError(new TerminalOperationError({ code, message: 'No open terminal.', retryable: false })));
    expect(screen.queryByText('No open terminal.')).toBeNull(); expect(f.model.report).not.toHaveBeenCalled();
    expect(f.model.refresh).toHaveBeenCalledWith(f.client);
  });
});
