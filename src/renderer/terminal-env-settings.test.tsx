// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { afterEach, expect, it } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { failure } from '../shared/contracts';
import { SettingsPanel } from './SettingsPanel';
import { createManagerClient } from './store';
import { mockApi, profiles, snapshot } from './test-fixtures';
import { openSettingsTab } from './settings-test-navigation';

afterEach(cleanup);
function setup() {
  const initial = snapshot();
  initial.settings.terminalEnv = [{ name: 'SHARED', value: 'original' }];
  const fixture = mockApi(initial);
  const client = createManagerClient(fixture.api);
  render(<SettingsPanel client={client} settings={initial.settings} profiles={profiles} defaultProfileId="pwsh" />);
  openSettingsTab('Terminals');
  fireEvent.click(screen.getByRole('button', { name: 'Add Environment Variables (all terminals)' }));
  return fixture;
}
it('reuses the environment editor and saves shared values without changing other settings', async () => {
  const fixture = setup();
  const dialog = screen.getByRole('dialog');
  expect(within(dialog).getByLabelText('Environment variables')).toHaveValue('SHARED=original');
  expect(dialog).toHaveTextContent('Session variables override these values.');
  fireEvent.change(within(dialog).getByLabelText('Environment variables'), { target: { value: 'SHARED=updated\nEMPTY=' } });
  fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  expect(fixture.api.setTerminalEnv).toHaveBeenCalledWith({ env: [{ name: 'SHARED', value: 'updated' }, { name: 'EMPTY', value: '' }] });
  expect(fixture.api.saveSettings).not.toHaveBeenCalled();
});
it('discards cancelled edits', () => {
  const fixture = setup();
  fireEvent.change(screen.getByLabelText('Environment variables'), { target: { value: 'SHARED=discarded' } });
  fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  expect(fixture.api.saveSettings).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'Add Environment Variables (all terminals)' }));
  expect(screen.getByLabelText('Environment variables')).toHaveValue('SHARED=original');
});
it('keeps failed saves open for correction and retry', async () => {
  const fixture = setup();
  fixture.api.setTerminalEnv.mockResolvedValueOnce(failure('STORAGE_FAILED', 'Disk is full.'));
  fireEvent.change(screen.getByLabelText('Environment variables'), { target: { value: 'SHARED=updated' } });
  fireEvent.click(screen.getByRole('button', { name: 'Save' }));
  await waitFor(() => expect(within(screen.getByRole('dialog')).getByRole('alert')).toHaveTextContent('Disk is full.'));
  expect(screen.getByLabelText('Environment variables')).toHaveValue('SHARED=updated');
  fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Save' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  expect(fixture.api.setTerminalEnv).toHaveBeenCalledTimes(2);
});
it('does not queue failed environment edits when cancelled before another settings autosave', async () => {
  const fixture = setup();
  fixture.api.setTerminalEnv.mockResolvedValueOnce(failure('STORAGE_FAILED', 'Disk is full.'));
  fireEvent.change(screen.getByLabelText('Environment variables'), { target: { value: 'SHARED=discarded' } });
  fireEvent.click(screen.getByRole('button', { name: 'Save' }));
  await waitFor(() => expect(within(screen.getByRole('dialog')).getByRole('alert')).toBeVisible());
  fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
  openSettingsTab('General');
  fireEvent.change(screen.getByLabelText('Accent color'), { target: { value: '#22aa88' } });
  await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalled());
  expect(fixture.api.saveSettings.mock.calls.at(-1)![0]).not.toHaveProperty('terminalEnv');
  openSettingsTab('Terminals');
  fireEvent.click(screen.getByRole('button', { name: 'Add Environment Variables (all terminals)' }));
  expect(screen.getByLabelText('Environment variables')).toHaveValue('SHARED=original');
});
it('rejects duplicate names before sending a save', () => {
  const fixture = setup();
  fireEvent.change(screen.getByLabelText('Environment variables'), { target: { value: 'SHARED=one\nSHARED=two' } });
  expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();
  expect(fixture.api.saveSettings).not.toHaveBeenCalled();
});
