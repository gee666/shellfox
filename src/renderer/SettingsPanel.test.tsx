// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { failure, success } from '../shared/contracts';
import { processRuleSchema } from '../shared/schemas';
import { SettingsPanel, createAgentRule } from './SettingsPanel';
import { createManagerClient } from './store';
import { deferred, mockApi, profiles, snapshot } from './test-fixtures';

afterEach(cleanup);
function setup(initial = snapshot()) {
  const fixture = mockApi(initial); const client = createManagerClient(fixture.api);
  const user = userEvent.setup();
  const rendered = render(<SettingsPanel client={client} settings={initial.settings} explorer={initial.explorer} cli={initial.cli} profiles={profiles} defaultProfileId="pwsh" />);
  return { ...rendered, fixture, client, user };
}
describe('autosaving settings', () => {
  it('debounces color changes and preserves hidden legacy fields', async () => {
    const initial = snapshot(); initial.settings.historyPageSize = 37; initial.settings.shellExecutable = 'C:\\legacy\\shell.exe';
    const { fixture } = setup(initial);
    fireEvent.change(screen.getByLabelText('Accent color'), { target: { value: '#22aa88' } });
    fireEvent.change(screen.getByLabelText('Accent color'), { target: { value: '#60a5fa' } });
    expect(fixture.api.saveSettings).not.toHaveBeenCalled();
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalledTimes(1));
    expect(fixture.api.saveSettings.mock.calls[0]![0]).toMatchObject({ accentColor: '#60a5fa', historyPageSize: 37, shellExecutable: 'C:\\legacy\\shell.exe', adapterId: initial.settings.adapterId, shellId: initial.settings.shellId });
    expect(screen.getByRole('status')).toHaveTextContent('Saved');
    expect(screen.queryByLabelText('History page size')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Save settings' })).not.toBeInTheDocument();
  });
  it('saves the default shell from discovered profiles', async () => {
    const { fixture, user } = setup();
    await user.selectOptions(screen.getByLabelText('Default shell'), 'wsl:Ubuntu');
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ terminalProfileId: 'wsl:Ubuntu' })));
  });
  it('adds an agent by name with Enter and creates executable names without exposing ids', async () => {
    const { fixture, user } = setup();
    await user.type(screen.getByLabelText('Add agent'), 'aider{Enter}');
    expect(screen.getByRole('switch', { name: 'Track aider' })).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByText('aider, aider.exe')).toBeVisible();
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalled());
    const rule = fixture.api.saveSettings.mock.calls.at(-1)![0].processRules.at(-1)!;
    expect(rule).toMatchObject({ label: 'aider', enabled: true, executableBasenames: ['aider', 'aider.exe'], executablePaths: [], scriptPathSuffixes: [] });
    expect(rule.id).toMatch(/^custom-aider-/); expect(screen.queryByText(rule.id)).not.toBeInTheDocument();
    expect(createAgentRule('agent.exe').executableBasenames).toEqual(['agent.exe']);
    expect(processRuleSchema.safeParse(rule).success).toBe(true);
  });
  it('rejects paths/commands and duplicate names without saving', async () => {
    const { fixture, user } = setup();
    await user.type(screen.getByLabelText('Add agent'), 'C:\\tools\\aider.exe{Enter}');
    expect(screen.getByRole('alert')).toHaveTextContent('program name');
    await user.clear(screen.getByLabelText('Add agent'));
    await user.type(screen.getByLabelText('Add agent'), 'node.exe{Enter}');
    expect(screen.getByRole('alert')).toHaveTextContent('already listed');
    expect(fixture.api.saveSettings).not.toHaveBeenCalled();
  });
  it('protects built-in rules from removal and lets users toggle them', async () => {
    const initial = snapshot(); initial.settings.processRules[0]!.id = 'f919fb1a-fb03-4a93-8b9b-1cde465d5870';
    const { fixture, user } = setup(initial);
    expect(screen.queryByRole('button', { name: 'Remove Pi' })).not.toBeInTheDocument();
    await user.click(screen.getByRole('switch', { name: 'Track Pi' }));
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalled());
    expect(fixture.api.saveSettings.mock.calls.at(-1)![0].processRules[0]!.enabled).toBe(false);
    expect(fixture.api.saveSettings.mock.calls.at(-1)![0].processRules[0]!.scriptPathSuffixes).toEqual(initial.settings.processRules[0]!.scriptPathSuffixes);
  });
  it('retains comma-separated advanced input while typing and validates paths inline', async () => {
    const { fixture, user } = setup();
    await user.click(screen.getByText('Advanced'));
    const input = screen.getByLabelText('Executable paths for Pi');
    await user.type(input, 'relative');
    expect(screen.getByRole('alert')).toHaveTextContent('absolute local paths');
    await new Promise(resolve => setTimeout(resolve, 450)); expect(fixture.api.saveSettings).not.toHaveBeenCalled();
    await user.clear(input);
    await user.type(input, 'C:\\tools\\one.exe, C:\\tools\\two.exe');
    expect(input).toHaveValue('C:\\tools\\one.exe, C:\\tools\\two.exe');
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalled());
    expect(fixture.api.saveSettings.mock.calls.at(-1)![0].processRules[0]!.executablePaths).toEqual(['C:\\tools\\one.exe', 'C:\\tools\\two.exe']);
  });
  it('removes user-added rules', async () => {
    const { fixture, user } = setup();
    await user.click(screen.getByRole('button', { name: 'Remove Pi' }));
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ processRules: [] })));
  });
  it('serializes autosaves and keeps later edits made during a pending save', async () => {
    const { fixture, user } = setup();
    const pending = deferred<Awaited<ReturnType<typeof fixture.api.saveSettings>>>();
    fixture.api.saveSettings.mockReturnValueOnce(pending.promise);
    await user.click(screen.getByRole('button', { name: 'Use #60a5fa' }));
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalledTimes(1));
    await user.click(screen.getByRole('switch', { name: 'Track Pi' }));
    await new Promise(resolve => setTimeout(resolve, 450)); expect(fixture.api.saveSettings).toHaveBeenCalledTimes(1);
    await act(async () => pending.resolve(success(fixture.api.saveSettings.mock.calls[0]![0])));
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalledTimes(2));
    expect(fixture.api.saveSettings.mock.calls[1]![0].processRules[0]!.enabled).toBe(false);
  });
  it('flushes unsaved edits on modal unmount', async () => {
    const { fixture, unmount } = setup();
    fireEvent.change(screen.getByLabelText('Accent color'), { target: { value: '#60a5fa' } });
    unmount(); await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalledTimes(1));
  });
  it('shows a backend validation error next to the changed field', async () => {
    const { fixture, user } = setup(); fixture.api.saveSettings.mockResolvedValueOnce(failure('VALIDATION', 'Unsupported shell.'));
    await user.selectOptions(screen.getByLabelText('Default shell'), 'wsl:Ubuntu');
    const error = await screen.findByRole('alert'); expect(error).toHaveTextContent('Unsupported shell.');
    expect(within(error.parentElement!).getByLabelText('Default shell')).toBeVisible();
  });
  it('reopens the latest draft and coalesces pending saves across modal mounts', async () => {
    const initial = snapshot();
    const { fixture, client, unmount, user } = setup(initial);
    const pending = deferred<Awaited<ReturnType<typeof fixture.api.saveSettings>>>();
    fixture.api.saveSettings.mockReturnValueOnce(pending.promise);
    await user.click(screen.getByLabelText('Use #60a5fa'));
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalledTimes(1));
    await user.click(screen.getByRole('switch', { name: 'Track Pi' })); unmount();
    const second = render(<SettingsPanel client={client} settings={initial.settings} explorer={initial.explorer} cli={initial.cli} profiles={profiles} defaultProfileId="pwsh" />);
    expect(screen.getByLabelText('Use #60a5fa')).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('switch', { name: 'Track Pi' })).toHaveAttribute('aria-checked', 'false');
    await user.click(screen.getByLabelText('Use #4ade80'));
    await new Promise(resolve => setTimeout(resolve, 450));
    expect(fixture.api.saveSettings).toHaveBeenCalledTimes(1);
    await act(async () => pending.resolve(success(fixture.api.saveSettings.mock.calls[0]![0])));
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalledTimes(2));
    expect(fixture.api.saveSettings.mock.calls[1]![0].accentColor).toBe('#4ade80');
    expect(fixture.api.saveSettings.mock.calls[1]![0].processRules[0]!.enabled).toBe(false);
    second.unmount(); expect(fixture.api.saveSettings).toHaveBeenCalledTimes(2);
  });
  it('rebases canonical shell fields when switching profiles repeatedly without closing', async () => {
    const { fixture, client, user } = setup();
    fixture.api.saveSettings.mockImplementation(async value => {
      const profile = profiles.find(profile => profile.id === value.terminalProfileId)!;
      return success({ ...value, shellId: profile.id === 'pwsh' ? 'pwsh' : 'wsl', shellExecutable: profile.executable });
    });
    await user.selectOptions(screen.getByLabelText('Default shell'), 'wsl:Ubuntu');
    await waitFor(() => expect(client.settings.store.getState().draft?.shellExecutable).toBe(profiles[1]!.executable));
    await user.selectOptions(screen.getByLabelText('Default shell'), 'pwsh');
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalledTimes(2));
    expect(fixture.api.saveSettings.mock.calls[1]![0]).toMatchObject({ terminalProfileId: 'pwsh', shellId: 'wsl', shellExecutable: profiles[1]!.executable });
    await waitFor(() => expect(client.settings.store.getState().draft?.shellExecutable).toBe(profiles[0]!.executable));
    await user.click(screen.getByRole('switch', { name: 'Track Pi' }));
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalledTimes(3));
    expect(fixture.api.saveSettings.mock.calls[2]![0]).toMatchObject({ shellId: 'pwsh', shellExecutable: profiles[0]!.executable });
  });
  it('preserves an external hidden-field update while the accent draft is dirty', async () => {
    const initial = snapshot(); const { fixture, client, rerender } = setup(initial);
    fireEvent.click(screen.getByLabelText('Use #60a5fa'));
    const external = { ...initial.settings, historyPageSize: 3 };
    rerender(<SettingsPanel client={client} settings={external} explorer={initial.explorer} cli={initial.cli} profiles={profiles} defaultProfileId="pwsh" />);
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalledTimes(1));
    expect(fixture.api.saveSettings.mock.calls[0]![0]).toMatchObject({ accentColor: '#60a5fa', historyPageSize: 3 });
  });
  it('updates clean advanced text from authoritative props and preserves partial comma typing', async () => {
    const initial = snapshot(); const { client, rerender, user } = setup(initial);
    await user.click(screen.getByText('Advanced'));
    const external = { ...initial.settings, processRules: [{ ...initial.settings.processRules[0]!, executablePaths: ['C:\\external\\pi.exe'] }] };
    rerender(<SettingsPanel client={client} settings={external} explorer={initial.explorer} cli={initial.cli} profiles={profiles} defaultProfileId="pwsh" />);
    const input = screen.getByLabelText('Executable paths for Pi');
    await waitFor(() => expect(input).toHaveValue('C:\\external\\pi.exe'));
    await user.type(input, ', ');
    expect(input).toHaveValue('C:\\external\\pi.exe, ');
  });
  it('toggles the Terminal command using the CLI integration API and reflects authoritative updates', async () => {
    const initial = snapshot(); const { fixture, client, user, rerender } = setup(initial);
    expect(screen.getByRole('heading', { name: 'Terminal command' })).toBeVisible();
    expect(screen.getByText('shellfox start .')).toBeVisible();
    const toggle = screen.getByRole('switch', { name: 'Enable shellfox start <path> in terminals' });
    await user.click(toggle);
    expect(fixture.api.setCliIntegration).toHaveBeenCalledWith({ installed: true });
    await waitFor(() => expect(toggle).toHaveAttribute('aria-checked', 'true'));
    await user.click(toggle);
    expect(fixture.api.setCliIntegration).toHaveBeenLastCalledWith({ installed: false });
    await waitFor(() => expect(toggle).toHaveAttribute('aria-checked', 'false'));
    expect(fixture.api.saveSettings).not.toHaveBeenCalled();
    rerender(<SettingsPanel client={client} settings={initial.settings} explorer={initial.explorer} cli={{ ...initial.cli, installed: true }} profiles={profiles} defaultProfileId="pwsh" />);
    expect(toggle).toHaveAttribute('aria-checked', 'true');
  });
  it('disables an unsupported Terminal command and shows the supplied reason', async () => {
    const initial = snapshot(); initial.cli = { ...initial.cli, supported: false, reason: 'Windows integration only.' };
    const { fixture, user } = setup(initial);
    const toggle = screen.getByRole('switch', { name: 'Enable shellfox start <path> in terminals' });
    expect(toggle).toBeDisabled(); expect(screen.getByText('Windows integration only.')).toBeVisible();
    await user.click(toggle); expect(fixture.api.setCliIntegration).not.toHaveBeenCalled();
  });
  it('shows integration guidance and startup errors even when Windows integration is supported', () => {
    const initial = snapshot();
    initial.explorer = { ...initial.explorer, supported: true, reason: 'Windows 11: use Show more options (Shift+F10) in Explorer.' };
    initial.cli = { ...initial.cli, supported: true, reason: 'Shellfox user PATH access failed.' };
    setup(initial);
    expect(screen.getByText(initial.explorer.reason!)).toBeVisible();
    expect(screen.getByText(initial.cli.reason!)).toBeVisible();
  });
  it('uses one Explorer switch and disables it with a reason when unsupported', async () => {
    const initial = snapshot(); initial.explorer.supported = false; initial.explorer.reason = 'Windows only.';
    const { user, fixture, rerender, client } = setup(initial);
    const toggle = screen.getByRole('switch', { name: /Explorer/ });
    expect(toggle).toBeDisabled(); expect(screen.getByText('Windows only.')).toBeVisible();
    rerender(<SettingsPanel client={client} settings={initial.settings} explorer={snapshot().explorer} cli={initial.cli} profiles={profiles} defaultProfileId="pwsh" />);
    await user.click(toggle); expect(fixture.api.setExplorerIntegration).toHaveBeenCalledWith({ installed: true });
    await waitFor(() => expect(toggle).toHaveAttribute('aria-checked', 'true'));
  });
});
