// @vitest-environment jsdom
import React, { type ReactNode } from 'react';
import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { failure, success, type EnvVar } from '../../src/shared/contracts';
import { snapshot } from '../../src/renderer/test-fixtures';
import { SettingsScreen } from './SettingsScreen';
import { RemoteClient } from './client/client';
import { deferred, ids, TestTransport } from './client/test-fixtures';
import { defaultPreferences } from './preferences';
import type { MachineView, ShellfoxModel } from './useShellfox';

const native = vi.hoisted(() => ({ platform: { OS: 'ios' }, back: () => {} }));
vi.mock('@react-native-async-storage/async-storage', () => ({ default: {} }));
vi.mock('react-native', () => ({
  Alert: { alert: vi.fn() }, Platform: native.platform,
  ScrollView: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  KeyboardAvoidingView: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Modal: ({ children, onRequestClose }: { children?: ReactNode; onRequestClose: () => void }) => {
    native.back = onRequestClose;
    return <div role="dialog">{children}</div>;
  },
}));
vi.mock('./ui', () => ({
  useUi: () => ({ colors: {}, styles: {} }),
  Button: ({ title, disabled, onPress }: { title: string; disabled?: boolean; onPress: () => void }) => <button disabled={disabled} onClick={onPress}>{title}</button>,
  Card: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Note: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  Field: ({ label, value, onChangeText, editable }: { label: string; value: string; onChangeText: (text: string) => void; editable?: boolean }) =>
    <input aria-label={label} value={value} disabled={editable === false} onChange={event => onChangeText(event.target.value)} />,
}));
vi.mock('./browser-login', () => ({ useBrowserLogin: () => ({ pending: false, start: vi.fn() }) }));
vi.mock('./updates/native', () => ({ updates: {} }));
vi.mock('./ColorSwatches', () => ({ ColorSwatches: () => null }));
vi.mock('./ModifierReleaseDelaySlider', () => ({ ModifierReleaseDelaySlider: () => null }));
vi.mock('./ConnectionStatus', () => ({ ConnectionStatus: () => null }));
vi.mock('./StatusIndicator', () => ({ StatusIndicator: () => null }));
afterEach(cleanup);
const control = 'Add Environment Variables (all terminals)';
function machine(id: string, env: EnvVar[] = []): MachineView {
  const client = new RemoteClient(id, new TestTransport(), { createClientId: () => ids.client });
  const hostSnapshot = snapshot([]);
  hostSnapshot.settings = { ...hostSnapshot.settings, terminalEnv: env };
  return {
    peer: { id, hostname: id, online: true, addresses: [] }, client,
    state: { peerId: id, status: 'connected', lastError: null, connectionId: `${id}:1` },
    snapshot: hostSnapshot, snapshotConnectionId: `${id}:1`, busyTabIds: {},
  };
}
function setup(machines: MachineView[]) {
  const model: ShellfoxModel = {
    ready: true, preferencesLoaded: true, loading: false,
    preferences: { ...defaultPreferences, remembered: machines.map(host => host.peer.id) },
    network: { status: 'online', loginUrl: null, error: null },
    peers: machines.map(host => host.peer), machines, error: null,
    setError: vi.fn(), report: vi.fn(), updatePreferences: vi.fn(), connect: vi.fn(), disconnect: vi.fn(),
    login: vi.fn(), logout: vi.fn(), renameDevice: vi.fn(), refresh: vi.fn(async () => {}), retryStart: vi.fn(),
  };
  const view = render(<SettingsScreen model={model} prompt={vi.fn()} />);
  return { model, rerender: () => view.rerender(<SettingsScreen model={model} prompt={vi.fn()} />) };
}
function change(label: string, value: string) {
  fireEvent.change(screen.getByLabelText(label), { target: { value } });
}

describe.each(['android', 'ios'])('host environment settings on %s', os => {
  it('edits only the selected host via the narrow API and refreshes it', async () => {
    native.platform.OS = os;
    const first = machine('linux', [{ name: 'MODE', value: 'one' }]);
    const second = machine('windows', [{ name: 'MODE', value: 'two' }]);
    const saveFirst = vi.spyOn(first.client, 'setTerminalEnv');
    const saveSecond = vi.spyOn(second.client, 'setTerminalEnv').mockResolvedValue(success(second.snapshot!.settings));
    const { model } = setup([first, second]);
    fireEvent.click(screen.getAllByRole('button', { name: control })[1]);
    expect(screen.getByText('Environment on windows')).toBeInTheDocument();
    expect(screen.getByText(/Applies to new terminals on windows/)).toBeInTheDocument();
    expect(screen.getByLabelText('Value')).toHaveValue('two');
    change('Value', 'literal $() ; 雪 = C:\\work');
    fireEvent.click(screen.getByRole('button', { name: 'Add variable' }));
    change('Variable 2', ' EMPTY ');
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(saveSecond).toHaveBeenCalledExactlyOnceWith({ env: [
      { name: 'MODE', value: 'literal $() ; 雪 = C:\\work' }, { name: 'EMPTY', value: '' },
    ] });
    expect(saveFirst).not.toHaveBeenCalled();
    expect(model.refresh).toHaveBeenCalledExactlyOnceWith(second.client);
    expect(model.updatePreferences).not.toHaveBeenCalled();
    expect(second.snapshot!.settings.terminalEnv).toEqual([{ name: 'MODE', value: 'two' }]);
  });

  it('supports clearing all globals and cancelling without changing saved values', async () => {
    native.platform.OS = os;
    const host = machine('mac', [{ name: 'MODE', value: 'old' }]);
    const save = vi.spyOn(host.client, 'setTerminalEnv').mockResolvedValue(success(host.snapshot!.settings));
    setup([host]);
    fireEvent.click(screen.getByRole('button', { name: control }));
    change('Value', 'discard');
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(save).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: control }));
    expect(screen.getByLabelText('Value')).toHaveValue('old');
    fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(save).toHaveBeenCalledExactlyOnceWith({ env: [] });
  });
});

it('hides host editing until connected with a snapshot from the current connection', () => {
  const host = machine('host');
  host.state.status = 'awaiting-approval';
  const { rerender } = setup([host]);
  expect(screen.queryByRole('button', { name: control })).not.toBeInTheDocument();
  host.state.status = 'reconnecting'; rerender();
  expect(screen.queryByRole('button', { name: control })).not.toBeInTheDocument();
  host.state.status = 'connected'; host.snapshotConnectionId = 'old'; rerender();
  expect(screen.queryByRole('button', { name: control })).not.toBeInTheDocument();
  host.snapshotConnectionId = host.state.connectionId; rerender();
  fireEvent.click(screen.getByRole('button', { name: control }));
  fireEvent.click(screen.getByRole('button', { name: 'Add variable' }));
  change('Variable 1', 'UNSAVED');
  host.state.connectionId = 'host:2'; host.snapshotConnectionId = 'host:2'; rerender();
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: control }));
  expect(screen.queryByLabelText('Variable 1')).not.toBeInTheDocument();
  host.state.status = 'disconnected'; rerender();
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
});

it('validates edits, blocks duplicate submissions, retains failures, and can retry', async () => {
  const host = machine('host');
  const pending = deferred<Awaited<ReturnType<RemoteClient['setTerminalEnv']>>>();
  const save = vi.spyOn(host.client, 'setTerminalEnv').mockReturnValueOnce(pending.promise)
    .mockResolvedValueOnce(success(host.snapshot!.settings));
  const { model } = setup([host]);
  fireEvent.click(screen.getByRole('button', { name: control }));
  fireEvent.click(screen.getByRole('button', { name: 'Add variable' }));
  change('Variable 1', 'BAD=NAME');
  fireEvent.click(screen.getByRole('button', { name: 'Save' }));
  expect(save).not.toHaveBeenCalled();
  expect(screen.getByText(/Names cannot contain/)).toBeInTheDocument();
  change('Variable 1', 'MODE');
  change('Value', 'keep draft');
  fireEvent.click(screen.getByRole('button', { name: 'Save' }));
  expect(screen.getByRole('button', { name: 'Saving...' })).toBeDisabled();
  expect(screen.getByRole('button', { name: 'Cancel' })).toBeDisabled();
  expect(screen.getByLabelText('Value')).toBeDisabled();
  act(() => native.back());
  expect(screen.getByRole('dialog')).toBeInTheDocument();
  await act(async () => pending.resolve(failure('VALIDATION', 'Upgrade the host to edit global environment variables.')));
  expect(screen.getByText('Upgrade the host to edit global environment variables.')).toBeInTheDocument();
  expect(screen.getByLabelText('Value')).toHaveValue('keep draft');
  expect(model.refresh).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'Save' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  expect(save).toHaveBeenCalledTimes(2);
});
