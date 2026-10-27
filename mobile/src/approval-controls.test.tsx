// @vitest-environment jsdom
import React, { type ReactNode } from 'react';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { RemoteClient } from './client/client';
import { TestTransport } from './client/test-fixtures';
import type { MachineView, ShellfoxModel } from './useShellfox';

interface Props { children?: ReactNode; title?: string; onPress?: () => void; disabled?: boolean; accessibilityLabel?: string }
vi.mock('react-native', () => ({
  Platform: { OS: 'ios' }, Alert: { alert: vi.fn() }, AppState: { currentState: 'active' }, Linking: { openURL: vi.fn() },
  Text: ({ children }: Props) => <span>{children}</span>, View: ({ children }: Props) => <div>{children}</div>,
  ScrollView: ({ children }: Props) => <div>{children}</div>, TextInput: () => null,
  Pressable: ({ children, onPress, accessibilityLabel }: Props) => <button aria-label={accessibilityLabel} onClick={onPress}>{children}</button>,
  StyleSheet: { create: <T,>(styles: T) => styles },
  SectionList: ({ sections, renderSectionHeader }: { sections: { machine: MachineView }[]; renderSectionHeader: (value: { section: { machine: MachineView } }) => ReactNode }) => <>{sections.map(section => <div key={section.machine.peer.id}>{renderSectionHeader({ section })}</div>)}</>,
}));
vi.mock('./ui', () => ({
  useUi: () => ({ colors: {}, styles: {} }),
  Button: ({ title, onPress, disabled }: Props) => <button disabled={disabled} onClick={onPress}>{title}</button>,
  Card: ({ children }: Props) => <div>{children}</div>, Note: ({ children }: Props) => <span>{children}</span>, Field: () => null,
}));
vi.mock('./updates/native', () => ({ updates: {} }));
vi.mock('./ColorSwatches', () => ({ ColorSwatches: () => null }));
vi.mock('./StatusIndicator', () => ({ StatusIndicator: () => null }));
vi.mock('./DirectoryPrompt', () => ({ DirectoryPrompt: () => null }));
vi.mock('./EnvironmentEditor', () => ({ EnvironmentEditor: () => null }));
vi.mock('./terminal/TerminalView', () => ({ TerminalView: () => null }));
vi.mock('./ActionMenu', () => ({ ActionMenu: () => null }));
const { SettingsScreen } = await import('./SettingsScreen');
const { SessionsScreen } = await import('./SessionsScreen');
const { SessionScreen } = await import('./SessionScreen');
afterEach(cleanup);
function fixture(): ShellfoxModel {
  const peer = { id: 'host', hostname: 'shellfox-omarchy-red', online: true, addresses: ['100.64.0.2'] };
  const client = new RemoteClient(peer.id, new TestTransport(), { createClientId: () => '10000000-0000-4000-8000-000000000001' });
  const machine: MachineView = { peer, client, snapshot: null, snapshotConnectionId: null, busyTabIds: {},
    state: { peerId: peer.id, status: 'awaiting-approval', connectionId: null, lastError: { code: 'APPROVAL_REQUIRED', message: 'Approve on host.', retryable: true } } };
  return { ready: true, loading: false, network: { status: 'online', loginUrl: null, error: null },
    peers: [peer], machines: [machine], preferences: { hostname: 'shellfox-mobile', remembered: [peer.id], fontSize: 14 }, error: null,
    setError: vi.fn(), report: vi.fn(), updatePreferences: vi.fn(), connect: vi.fn(), disconnect: vi.fn(async () => {}),
    login: vi.fn(), logout: vi.fn(), renameDevice: vi.fn(), refresh: vi.fn(), retryStart: vi.fn() };
}
function ui(name: string, model: ShellfoxModel) {
  if (name === 'settings') return <SettingsScreen model={model} prompt={vi.fn()} />;
  if (name === 'sessions') return <SessionsScreen model={model} open={vi.fn()} prompt={vi.fn()} settings={vi.fn()} />;
  return <SessionScreen model={model} machine={model.machines[0]} sessionId="session" back={vi.fn()} prompt={vi.fn()} />;
}
it.each(['settings', 'sessions', 'terminal'])('offers Cancel request on %s and uses model disconnect rather than reconnect or forget', async name => {
  const model = fixture(); render(ui(name, model));
  expect(screen.queryByRole('button', { name: 'Reconnect' })).toBeNull();
  expect(screen.queryByRole('button', { name: 'Connect' })).toBeNull();
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Cancel request' })); });
  expect(model.disconnect).toHaveBeenCalledExactlyOnceWith('host');
  expect(model.connect).not.toHaveBeenCalled(); expect(model.report).not.toHaveBeenCalled();
});
it('keeps a saved pending request cancellable when discovery temporarily omits the host', async () => {
  const model = fixture(); model.peers = []; render(ui('settings', model));
  expect(screen.getByText('shellfox-omarchy-red')).toBeTruthy();
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Cancel request' })); });
  expect(model.disconnect).toHaveBeenCalledExactlyOnceWith('host');
});
it.each(['settings', 'sessions', 'terminal'])('reports a failed cancellation on %s', async name => {
  const model = fixture(), error = new Error('Could not save preferences');
  vi.mocked(model.disconnect).mockRejectedValueOnce(error); render(ui(name, model));
  await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Cancel request' })); });
  expect(model.report).toHaveBeenCalledWith(error);
});
