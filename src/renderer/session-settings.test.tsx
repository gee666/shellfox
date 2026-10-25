// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { afterEach, expect, it } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { failure } from '../shared/contracts';
import { SettingsPanel } from './SettingsPanel';
import { validateSettingsDraft } from './settings-controller';
import { createManagerClient } from './store';
import { mockApi, profiles, settings, snapshot } from './test-fixtures';

afterEach(cleanup);
function setup(platform = 'win32') {
  const initial = snapshot(); initial.probe.platform = platform;
  const fixture = mockApi(initial); const client = createManagerClient(fixture.api);
  client.store.setState({ snapshot: initial });
  render(<SettingsPanel client={client} settings={initial.settings} explorer={initial.explorer} cli={initial.cli} profiles={profiles} defaultProfileId="pwsh" />);
  return fixture;
}

it.each(['win32', 'linux', 'darwin'])('configures pagination and session timers on %s', async platform => {
  const fixture = setup(platform);
  expect(screen.getByLabelText('Sessions per page')).toHaveValue(10);
  expect(screen.getByLabelText('Settle after (hours)')).toHaveValue(24);
  expect(screen.getByLabelText('Delete settled after (hours)')).toHaveValue(720);
  fireEvent.change(screen.getByLabelText('Sessions per page'), { target: { value: '5' } });
  fireEvent.change(screen.getByLabelText('Settle after (hours)'), { target: { value: '12.5' } });
  fireEvent.change(screen.getByLabelText('Delete settled after (hours)'), { target: { value: '48' } });
  await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalledTimes(1));
  expect(fixture.api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ historyPageSize: 5, sessionSettleHours: 12.5, sessionDeleteHours: 48 }));
});

it.each([
  ['historyPageSize', 0], ['historyPageSize', 101], ['historyPageSize', 1.5], ['historyPageSize', Number.NaN],
  ['sessionSettleHours', 0], ['sessionSettleHours', 87601], ['sessionSettleHours', Number.NaN],
  ['sessionDeleteHours', -1], ['sessionDeleteHours', 87601], ['sessionDeleteHours', Number.POSITIVE_INFINITY],
])('validates %s=%s before saving', (field, value) => {
  expect(validateSettingsDraft({ ...settings, [field]: value })[field]).toBeTruthy();
});

it('rejects empty and out-of-range fields locally without autosaving them', async () => {
  const fixture = setup();
  fireEvent.change(screen.getByLabelText('Sessions per page'), { target: { value: '' } });
  expect(screen.getByRole('alert')).toHaveTextContent('Use a whole number from 1 to 100.');
  fireEvent.change(screen.getByLabelText('Settle after (hours)'), { target: { value: '0' } });
  fireEvent.change(screen.getByLabelText('Delete settled after (hours)'), { target: { value: '87601' } });
  expect(screen.getAllByRole('alert')).toHaveLength(3);
  // A valid unrelated edit must not flush an invalid settings draft.
  fireEvent.change(screen.getByLabelText('Accent color'), { target: { value: '#60a5fa' } });
  await new Promise(resolve => setTimeout(resolve, 450));
  expect(fixture.api.saveSettings).not.toHaveBeenCalled();
});

it('shows a save failure next to the session field', async () => {
  const fixture = setup(); fixture.api.saveSettings.mockResolvedValueOnce(failure('STORAGE_FAILED', 'Could not save timers.'));
  fireEvent.change(screen.getByLabelText('Settle after (hours)'), { target: { value: '36' } });
  expect(await screen.findByRole('alert')).toHaveTextContent('Could not save timers.');
  expect(screen.getByRole('alert').previousElementSibling).toHaveTextContent('Settle after (hours)');
});
