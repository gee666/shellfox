// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { failure, success } from '../shared/contracts';
import { processRuleSchema } from '../shared/schemas';
import { defaultSettings } from '../main/defaults';
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
    expect(screen.getByText('Process name: aider, aider.exe')).toBeVisible();
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalled());
    const rule = fixture.api.saveSettings.mock.calls.at(-1)![0].processRules.at(-1)!;
    expect(rule).toMatchObject({ label: 'aider', enabled: true, executableBasenames: ['aider', 'aider.exe'], executablePaths: [], scriptPathSuffixes: [] });
    expect(rule.id).toMatch(/^custom-aider-/); expect(screen.queryByText(rule.id)).not.toBeInTheDocument();
    expect(createAgentRule('agent.exe').executableBasenames).toEqual(['agent', 'agent.exe']);
    expect(processRuleSchema.safeParse(rule).success).toBe(true);
  });
  it.each(['C:\\Program Files\\Agent\\agent.exe', '/opt/Agent Tools/agent', '\\\\wsl.localhost\\Ubuntu\\opt\\Agent Tools\\agent', '\\\\wsl$\\Debian\\opt\\agent'])('adds an exact executable path with spaces: %s', async path => {
    const { fixture, user } = setup();
    fireEvent.change(screen.getByLabelText('Add agent'), { target: { value: path } });
    await user.click(within(screen.getByLabelText('Add agent').closest('form')!).getByRole('button', { name: 'Add' }));
    expect(screen.getByText('Exact path: ' + path)).toBeVisible();
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalled());
    const rule = fixture.api.saveSettings.mock.calls.at(-1)![0].processRules.at(-1)!;
    expect(rule).toMatchObject({ enabled: true, executableBasenames: [], executablePaths: [path], scriptPathSuffixes: [] });
    expect(processRuleSchema.safeParse(rule).success).toBe(true);
  });
  it('accepts paths longer than the label limit and keeps the entire path', async () => {
    const path = '/opt/' + 'agent-tools/'.repeat(25) + 'agent';
    const { fixture, user } = setup();
    const input = screen.getByLabelText('Add agent');
    expect(input).toHaveAttribute('maxlength', '32760');
    fireEvent.change(input, { target: { value: path } });
    await user.click(within(screen.getByLabelText('Add agent').closest('form')!).getByRole('button', { name: 'Add' }));
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalled());
    const rule = fixture.api.saveSettings.mock.calls.at(-1)![0].processRules.at(-1)!;
    expect(rule.label).toBe('agent');
    expect(rule.executablePaths).toEqual([path]);
    expect(processRuleSchema.safeParse(rule).success).toBe(true);
  });
  it.each(['./agent', 'tools/agent', 'C:agent.exe', '\\\\server\\share\\agent.exe', '\\\\?\\C:\\agent.exe', 'https://example.com/agent', 'agent --flag', '"C:\\Program Files\\agent.exe"', 'agent\u0000', 'agent\u0001--flag', 'a'.repeat(201)])('rejects invalid input without saving: %j', async value => {
    const { fixture, user, client } = setup();
    fireEvent.change(screen.getByLabelText('Add agent'), { target: { value } });
    await user.click(within(screen.getByLabelText('Add agent').closest('form')!).getByRole('button', { name: 'Add' }));
    expect(screen.getByRole('alert')).toHaveTextContent('process name or an absolute local executable path');
    expect(screen.getByLabelText('Add agent')).toHaveAttribute('aria-invalid', 'true');
    expect(client.settings.store.getState().draft?.processRules).toHaveLength(1);
    client.settings.flush();
    expect(fixture.api.saveSettings).not.toHaveBeenCalled();
  });
  it('deduplicates process names case-insensitively and treats .exe as the same name', async () => {
    const initial = snapshot(); initial.settings.processRules = [createAgentRule('aider.exe')];
    const { fixture, user } = setup(initial);
    for (const name of ['aider', 'AIDER.EXE', ' aider ']) {
      fireEvent.change(screen.getByLabelText('Add agent'), { target: { value: name } });
      await user.click(within(screen.getByLabelText('Add agent').closest('form')!).getByRole('button', { name: 'Add' }));
      expect(screen.getByRole('alert')).toHaveTextContent('already listed');
    }
    expect(fixture.api.saveSettings).not.toHaveBeenCalled();
  });
  it.each(['bun', 'node'])('allows custom %s with actual bundled defaults and then rejects direct-name duplicates', async name => {
    const initial = snapshot(); initial.settings = structuredClone(defaultSettings);
    const { fixture, user, client } = setup(initial);
    await user.type(screen.getByLabelText('Add agent'), name + '{Enter}');
    expect(screen.getByRole('switch', { name: 'Track ' + name })).toBeVisible();
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalledTimes(1));
    const rules = fixture.api.saveSettings.mock.calls[0]![0].processRules;
    expect(rules.slice(0, -1)).toEqual(defaultSettings.processRules);
    expect(rules.at(-1)).toMatchObject({ executableBasenames: [name, name + '.exe'], executablePaths: [], scriptPathSuffixes: [] });
    await user.type(screen.getByLabelText('Add agent'), name.toUpperCase() + '.EXE{Enter}');
    expect(screen.getByRole('alert')).toHaveTextContent('already listed');
    client.settings.flush();
    expect(fixture.api.saveSettings).toHaveBeenCalledTimes(1);
  });
  it('does not deduplicate names against legacy script-constrained runtimes', async () => {
    const initial = snapshot(); initial.settings = structuredClone(defaultSettings);
    for (const rule of initial.settings.processRules) delete rule.processNames;
    const { fixture, user } = setup(initial);
    await user.type(screen.getByLabelText('Add agent'), 'node{Enter}');
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalledTimes(1));
    expect(fixture.api.saveSettings.mock.calls[0]![0].processRules.at(-1)!.executableBasenames).toEqual(['node', 'node.exe']);
  });
  it('does not deduplicate names against exact-path rules even with basenames and processNames', async () => {
    const initial = snapshot(); initial.settings = structuredClone(defaultSettings);
    initial.settings.processRules.push({ ...createAgentRule('/opt/bun'), executableBasenames: ['bun', 'bun.exe'], processNames: ['bun', 'bun.exe'] });
    const { fixture, user } = setup(initial);
    await user.type(screen.getByLabelText('Add agent'), 'bun{Enter}');
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalledTimes(1));
    expect(fixture.api.saveSettings.mock.calls[0]![0].processRules.at(-1)).toMatchObject({ executableBasenames: ['bun', 'bun.exe'], executablePaths: [] });
  });
  it('deduplicates Windows paths across case and slash variants without changing the stored path', async () => {
    const path = 'C:\\Program Files\\agent.exe';
    const initial = snapshot(); initial.settings.processRules = [createAgentRule(path)];
    const { fixture, user, client } = setup(initial);
    fireEvent.change(screen.getByLabelText('Add agent'), { target: { value: ' c:/PROGRAM FILES/AGENT.EXE ' } });
    await user.click(within(screen.getByLabelText('Add agent').closest('form')!).getByRole('button', { name: 'Add' }));
    expect(screen.getByRole('alert')).toHaveTextContent('already listed');
    expect(client.settings.store.getState().draft?.processRules[0]!.executablePaths).toEqual([path]);
    expect(fixture.api.saveSettings).not.toHaveBeenCalled();
  });
  it('keeps Linux paths case-sensitive and name trust separate from exact path trust', async () => {
    const initial = snapshot(); initial.settings.processRules = [createAgentRule('/opt/agent')];
    const { fixture, user } = setup(initial);
    const input = screen.getByLabelText('Add agent');
    fireEvent.change(input, { target: { value: '/opt/agent' } });
    await user.click(within(screen.getByLabelText('Add agent').closest('form')!).getByRole('button', { name: 'Add' }));
    expect(screen.getByRole('alert')).toHaveTextContent('already listed');
    for (const value of ['/opt/Agent', 'agent']) {
      fireEvent.change(input, { target: { value } });
      await user.click(within(screen.getByLabelText('Add agent').closest('form')!).getByRole('button', { name: 'Add' }));
      expect(input).toHaveValue('');
    }
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalled());
    expect(fixture.api.saveSettings.mock.calls.at(-1)![0].processRules).toHaveLength(3);
  });
  it('shows four standard built-in toggles without Advanced or removal and preserves hidden matcher fields', async () => {
    const initial = snapshot();
    initial.settings = structuredClone(defaultSettings);
    const { fixture, user } = setup(initial);
    expect(screen.getAllByRole('switch', { name: /^Track / })).toHaveLength(4);
    expect(screen.queryByRole('switch', { name: /Native/ })).not.toBeInTheDocument();
    expect(screen.queryByText('Advanced')).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/Executable paths for|Script path suffix for/)).not.toBeInTheDocument();
    for (const label of ['Pi', 'Claude', 'Codex', 'OpenCode']) {
      expect(screen.queryByRole('button', { name: 'Remove ' + label })).not.toBeInTheDocument();
      await user.click(screen.getByRole('switch', { name: 'Track ' + label }));
    }
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalled());
    expect(fixture.api.saveSettings.mock.calls.at(-1)![0].processRules).toEqual(initial.settings.processRules.map(rule => ({ ...rule, enabled: false })));
  });
  it.each(['PI.EXE', 'claude', 'CODEX', 'opencode.exe'])('rejects actual built-in process name %s', async name => {
    const initial = snapshot(); initial.settings = structuredClone(defaultSettings);
    const { fixture, user } = setup(initial);
    await user.type(screen.getByLabelText('Add agent'), name + '{Enter}');
    expect(screen.getByRole('alert')).toHaveTextContent('already listed');
    expect(fixture.api.saveSettings).not.toHaveBeenCalled();
  });
  it.each(['aider', '/opt/tools/agent'])('removes a custom name or path rule: %s', async value => {
    const initial = snapshot(); const custom = createAgentRule(value);
    initial.settings.processRules.push(custom);
    const { fixture, user } = setup(initial);
    await user.click(screen.getByRole('button', { name: 'Remove ' + custom.label }));
    expect(screen.queryByRole('switch', { name: 'Track ' + custom.label })).not.toBeInTheDocument();
    await waitFor(() => expect(fixture.api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ processRules: [initial.settings.processRules[0]] })));
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
  it('flushes unsaved edits on settings page unmount', async () => {
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
  it('reopens the latest draft and coalesces pending saves across settings page mounts', async () => {
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
  it('updates path summaries from authoritative props without disturbing the add-agent input', async () => {
    const initial = snapshot(); const { client, rerender, user } = setup(initial);
    await user.type(screen.getByLabelText('Add agent'), '/opt/partial path');
    const external = { ...initial.settings, processRules: [{ ...initial.settings.processRules[0]!, executablePaths: ['C:\\external\\pi.exe'] }] };
    rerender(<SettingsPanel client={client} settings={external} explorer={initial.explorer} cli={initial.cli} profiles={profiles} defaultProfileId="pwsh" />);
    await waitFor(() => expect(screen.getByText(/Exact path: C:\\external\\pi.exe/)).toBeVisible());
    expect(screen.getByLabelText('Add agent')).toHaveValue('/opt/partial path');
  });
  it('toggles the Terminal command using the CLI integration API and reflects authoritative updates', async () => {
    const initial = snapshot(); const { fixture, client, user, rerender } = setup(initial);
    expect(screen.getByRole('heading', { name: 'Terminal command' })).toBeVisible();
    expect(screen.getByText('shellfox start .')).toBeVisible();
    expect(screen.getAllByText('shellfox ssh')).toHaveLength(2);
    expect(screen.getByRole('region', { name: 'Settings' })).toBeVisible();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    const toggle = screen.getByRole('switch', { name: 'Enable shellfox command in terminals' });
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
    const toggle = screen.getByRole('switch', { name: 'Enable shellfox command in terminals' });
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
