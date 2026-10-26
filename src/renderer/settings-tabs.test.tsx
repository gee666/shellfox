// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import './terminal-test-mocks';
import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { failure } from '../shared/contracts';
import { SettingsPanel, type SettingsTab } from './SettingsPanel';
import { createManagerClient } from './store';
import { mockApi, profiles, snapshot } from './test-fixtures';
import { openSettingsTab } from './settings-test-navigation';
import { createRemoteController } from './remote-controller';
import { remoteFixture } from './remote-test-fixtures';

afterEach(cleanup);
function setup(activeTab?: SettingsTab) {
  const initial = snapshot(), fixture = mockApi(initial), client = createManagerClient(fixture.api);
  const onTabChange = vi.fn();
  const props = { client, settings: initial.settings, profiles, defaultProfileId: 'pwsh', activeTab, onTabChange };
  const view = render(<SettingsPanel {...props} />);
  return { ...view, fixture, client, props, onTabChange };
}

it('has five compact categories and exposes only the selected panel to assistive technology', () => {
  setup();
  expect(screen.getAllByRole('tab').map(tab => tab.textContent)).toEqual(['General', 'Terminals', 'Agents', 'Remote access', 'SSH']);
  expect(screen.getByRole('tab', { name: 'General' })).toHaveAttribute('aria-selected', 'true');
  expect(screen.getByRole('tabpanel', { name: 'General' })).toBeVisible();
  expect(screen.queryByRole('combobox', { name: 'Default shell' })).not.toBeInTheDocument();
  openSettingsTab('Terminals');
  expect(screen.getByRole('combobox', { name: 'Default shell' })).toBeVisible();
  expect(screen.queryByRole('tabpanel', { name: 'General' })).not.toBeInTheDocument();
});

it('separates remote access and SSH, with approval counts only on remote access', () => {
  const f = setup(), fixture = remoteFixture();
  fixture.state.pending = [1, 2].map(index => ({ id: `pending-${index}`, hostname: `Client ${index}`, addresses: [], online: true }));
  const remote = createRemoteController(fixture.api, f.fixture.api);
  remote.store.setState({ state: fixture.state });
  f.rerender(<SettingsPanel {...f.props} remote={remote} />);
  const tab = openSettingsTab('Remote access');
  expect(tab).toHaveTextContent('Remote access2');
  expect(tab.querySelector('[title="2 pending clients"]')).toBeInTheDocument();
  expect(screen.getByRole('switch', { name: 'Allow remote access' })).toBeVisible();
  expect(screen.queryByRole('heading', { name: 'SSH connections' })).not.toBeInTheDocument();
  expect(document.getElementById(tab.getAttribute('aria-controls')!)).toHaveAccessibleName('Remote access');
  const ssh = openSettingsTab('SSH');
  expect(ssh.querySelector('.settings-tab-count')).not.toBeInTheDocument();
  expect(screen.getByRole('heading', { name: 'SSH connections' })).toBeVisible();
  expect(screen.queryByRole('switch', { name: 'Allow remote access' })).not.toBeInTheDocument();
});

it('supports Arrow keys, Home and End with one tab stop', async () => {
  setup(); const user = userEvent.setup();
  screen.getByRole('tab', { name: 'General' }).focus();
  await user.keyboard('{ArrowRight}');
  expect(screen.getByRole('tab', { name: 'Terminals' })).toHaveFocus();
  expect(screen.getByRole('tab', { name: 'Terminals' })).toHaveAttribute('aria-selected', 'true');
  await user.keyboard('{End}'); expect(screen.getByRole('tab', { name: 'SSH' })).toHaveFocus();
  await user.keyboard('{ArrowRight}'); expect(screen.getByRole('tab', { name: 'General' })).toHaveFocus();
  await user.keyboard('{ArrowLeft}'); expect(screen.getByRole('tab', { name: 'SSH' })).toHaveFocus();
  await user.keyboard('{Home}'); expect(screen.getByRole('tab', { name: 'General' })).toHaveFocus();
  expect(screen.getAllByRole('tab').filter(tab => tab.tabIndex === 0)).toHaveLength(1);
});

it('retains form drafts across tabs while settings continue autosaving', async () => {
  const f = setup(), user = userEvent.setup();
  openSettingsTab('Agents'); await user.type(screen.getByLabelText('Add agent'), '/opt/unfinished agent');
  openSettingsTab('General');
  fireEvent.change(screen.getByLabelText('Accent color'), { target: { value: '#60a5fa' } });
  openSettingsTab('Terminals');
  await waitFor(() => expect(f.fixture.api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ accentColor: '#60a5fa' })));
  openSettingsTab('Agents'); expect(screen.getByLabelText('Add agent')).toHaveValue('/opt/unfinished agent');
  openSettingsTab('General'); expect(screen.getByLabelText('Accent color')).toHaveValue('#60a5fa');
});

it('marks a failed field on its category and keeps the error in the correct panel', async () => {
  const f = setup(); f.fixture.api.saveSettings.mockResolvedValueOnce(failure('VALIDATION', 'Shell is unavailable.'));
  openSettingsTab('Terminals');
  fireEvent.change(screen.getByLabelText('Default shell'), { target: { value: 'wsl:Ubuntu' } });
  openSettingsTab('General');
  await waitFor(() => expect(screen.getByRole('tab', { name: /Terminals, needs attention/ })).toBeVisible());
  expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  openSettingsTab('Terminals'); expect(screen.getByRole('alert')).toHaveTextContent('Shell is unavailable.');
});

it('allows the app to review Remote access without recreating drafts', () => {
  const f = setup('General');
  fireEvent.change(screen.getByLabelText('Accent color'), { target: { value: '#60a5fa' } });
  f.rerender(<SettingsPanel {...f.props} activeTab="Remote access" />);
  expect(screen.getByRole('tabpanel', { name: 'Remote access' })).toBeVisible();
  expect(f.client.settings.store.getState().draft?.accentColor).toBe('#60a5fa');
  fireEvent.click(screen.getByRole('tab', { name: 'Agents' }));
  expect(f.onTabChange).toHaveBeenCalledWith('Agents');
});
