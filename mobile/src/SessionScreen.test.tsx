// @vitest-environment jsdom
import React, { type ReactNode } from 'react';
import { cleanup, render, screen } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { session, snapshot } from '../../src/renderer/test-fixtures';
import { RemoteClient } from './client/client';
import { TestTransport } from './client/test-fixtures';
import type { MachineView, ShellfoxModel } from './useShellfox';
import { SessionScreen } from './SessionScreen';

type NativeProps = { children?: ReactNode; accessibilityLabel?: string; accessibilityRole?: string; disabled?: boolean; onPress?: () => void };
vi.mock('react-native', () => ({
  Alert: { alert: vi.fn() },
  Text: ({ children }: NativeProps) => <span>{children}</span>,
  View: ({ children }: NativeProps) => <div>{children}</div>,
  ScrollView: ({ children }: NativeProps) => <div>{children}</div>,
  Pressable: ({ children, accessibilityLabel, accessibilityRole, disabled, onPress }: NativeProps) => <button role={accessibilityRole} aria-label={accessibilityLabel} disabled={disabled} onClick={onPress}>{children}</button>,
}));
vi.mock('./ui', () => ({
  useUi: () => ({ colors: {}, styles: {} }),
  Button: ({ title, onPress }: { title: string; onPress: () => void }) => <button onClick={onPress}>{title}</button>,
  Note: ({ children }: NativeProps) => <span>{children}</span>,
}));
vi.mock('./terminal/TerminalView', () => ({ TerminalView: ({ tabId }: { tabId: string }) => <div data-testid="terminal" data-tab={tabId} /> }));
vi.mock('./ActionMenu', () => ({ ActionMenu: () => null }));
vi.mock('./StatusIndicator', () => ({ StatusIndicator: ({ kind, label }: { kind: string; label: string }) => <span role="img" aria-label={label} data-kind={kind} /> }));
afterEach(cleanup);
function setup(connected = true) {
  const item = session();
  item.tabs[0].status = 'running';
  const transport = new TestTransport();
  const client = new RemoteClient('host', transport, { createClientId: () => '10000000-0000-4000-8000-000000000001' });
  const machine: MachineView = {
    peer: { id: 'host', hostname: 'workstation', online: true, addresses: ['100.64.0.2'] },
    client, state: { ...client.getState(), status: connected ? 'connected' : 'disconnected', connectionId: 'test' },
    snapshot: snapshot([item]), busyTabIds: { [item.tabs[0].id]: true },
  };
  const model: ShellfoxModel = {
    ready: true, loading: false, network: { status: 'online', loginUrl: null, error: null }, peers: [], machines: [machine],
    preferences: { hostname: 'phone', fontSize: 14, remembered: [] }, error: null,
    setError: vi.fn(), report: vi.fn(), updatePreferences: vi.fn(), connect: vi.fn(), disconnect: vi.fn(),
    login: vi.fn(), logout: vi.fn(), renameDevice: vi.fn(), refresh: vi.fn(), retryStart: vi.fn(),
  };
  render(<SessionScreen machine={machine} sessionId={item.id} model={model} back={vi.fn()} prompt={vi.fn()} />);
  return item;
}
describe('compact terminal chrome', () => {
  it('keeps the session and host context with an accessible plus control and shared busy status', () => {
    const item = setup();
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
});
