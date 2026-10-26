// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import './terminal-test-mocks';
import { afterEach, describe, expect, it } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { failure, success } from '../shared/contracts';
import { responseSchemas } from '../shared/schemas';
import { App } from './App';
import { openSettingsTab } from './settings-test-navigation';
import { mockApi } from './test-fixtures';
import { remoteFixture } from './remote-test-fixtures';

afterEach(() => { cleanup(); delete window.shellfoxRemote; });
async function setup() {
  const local = mockApi(), remote = remoteFixture();
  window.shellfoxRemote = remote.api;
  const rendered = render(<App api={local.api} />), user = userEvent.setup();
  await screen.findByRole('button', { name: 'Select session Remote session' });
  await waitFor(() => expect(remote.request).toHaveBeenCalledWith({ peerId: 'workstation', method: 'getTerminalProfiles', payload: {} }));
  return { ...rendered, local, remote, user };
}

describe('remote machines', () => {
  it('shows foldable machine sections without a Remote parent and retains offline machines', async () => {
    const { user } = await setup();
    const sidebar = screen.getByRole('complementary', { name: 'Session navigation' });
    expect(within(sidebar).getByRole('button', { name: /Local · 1/ })).toHaveAttribute('aria-expanded', 'true');
    const machine = within(sidebar).getByRole('button', { name: /Workstation · 1/ });
    expect(machine).toHaveAttribute('aria-expanded', 'true');
    expect(within(sidebar).queryByRole('button', { name: /^Remote$/ })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Connect Linux server' })).toBeDisabled();
    await user.click(machine);
    expect(machine).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByRole('button', { name: 'Select session Remote session' })).not.toBeInTheDocument();
    await user.click(machine); expect(screen.getByRole('button', { name: 'Select session Remote session' })).toBeVisible();
  });

  it('isolates equal session and tab IDs and routes terminal actions to the selected host', async () => {
    const { local, remote, user } = await setup();
    await user.click(screen.getByRole('button', { name: 'Select session Remote session' }));
    expect(screen.getByRole('button', { name: 'Select session Session 1' })).toHaveAttribute('aria-pressed', 'false');
    expect(screen.getByRole('button', { name: 'Select session Remote session' })).toHaveAttribute('aria-pressed', 'true');
    await waitFor(() => expect(screen.getByRole('button', { name: 'New terminal' })).toBeEnabled());
    await user.click(screen.getByRole('button', { name: 'New terminal' }));
    expect(remote.request).toHaveBeenCalledWith({ peerId: 'workstation', method: 'addTab', payload: { sessionId: remote.remoteSnapshot.sessions[0]!.id, profileId: remote.remoteSnapshot.settings.terminalProfileId } });
    expect(local.api.addTab).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Select session Session 1' }));
    expect(screen.getByRole('button', { name: 'Select session Session 1' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('button', { name: 'Select session Remote session' })).toHaveAttribute('aria-pressed', 'false');
  });

  it('keeps same-ID terminal output separate and only detaches an attached remote terminal on disconnect', async () => {
    const { local, remote, user, unmount } = await setup();
    const item = remote.remoteSnapshot.sessions[0]!, tab = item.tabs[0]!;
    if (!tab.generation) throw new Error('Missing fixture generation');
    remote.request.mockResolvedValueOnce(responseSchemas.attachTerminal.parse(success({ tabId: tab.id, sessionId: item.id,
      generation: tab.generation, firstSequence: 1, lastSequence: 0, chunks: [], truncated: false, state: 'open', exitCode: null, cols: 80, rows: 24, lifetime: 'app-owned' })));
    await user.click(screen.getByRole('button', { name: /Workstation · 1/ }));
    await waitFor(() => expect(remote.request).toHaveBeenCalledWith(expect.objectContaining({ method: 'resizeTerminal' })));
    remote.request.mockResolvedValueOnce(success({ acknowledged: true as const }));
    await act(async () => {
      local.emitTerminal({ type: 'data', tabId: tab.id, generation: tab.generation!, sequence: 1, data: 'local-only' });
      remote.emit({ type: 'terminal', peerId: 'server', event: { type: 'data', tabId: tab.id, generation: tab.generation!, sequence: 1, data: 'wrong-machine' } });
      remote.emit({ type: 'terminal', peerId: 'workstation', event: { type: 'data', tabId: tab.id, generation: tab.generation!, sequence: 1, data: 'remote-only' } });
    });
    expect(await screen.findByText('remote-only')).toBeVisible();
    expect(screen.queryByText('local-only')).not.toBeInTheDocument(); expect(screen.queryByText('wrong-machine')).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Disconnect Workstation' }));
    await waitFor(() => expect(remote.request).toHaveBeenCalledWith({ peerId: 'workstation', method: 'detachTerminal', payload: { tabId: tab.id, generation: tab.generation } }));
    unmount();
    expect(remote.request.mock.calls.some(([input]) => input.method === 'closeTab' || input.method === 'deleteSession')).toBe(false);
  });

  it('chooses the target explicitly from Local and never creates on the wrong machine', async () => {
    const { local, remote, user } = await setup();
    remote.request.mockImplementation(async ({ method }) => {
      if (method === 'getSnapshot') return responseSchemas.getSnapshot.parse(success(remote.remoteSnapshot));
      if (method === 'getHomeDirectory') return success({ cwd: '/srv' });
      if (method === 'resolveDirectory') return success({ cwd: '/srv/new-project' });
      if (method === 'createSession') return responseSchemas.createSession.parse(success(remote.remoteSnapshot.sessions[0]!));
      return failure('UNSUPPORTED', 'Unavailable.');
    });
    await user.click(screen.getByRole('button', { name: 'New session' }));
    expect(screen.queryByRole('dialog', { name: 'New session' })).not.toBeInTheDocument();
    expect(local.api.createSession).not.toHaveBeenCalled();
    await user.click(screen.getByRole('menuitem', { name: /Workstation/ }));
    const input = await screen.findByRole('combobox', { name: 'Folder path' });
    expect(screen.getByText('Workstation')).toBeVisible(); expect(screen.queryByRole('button', { name: 'Browse folders' })).not.toBeInTheDocument();
    fireEvent.change(input, { target: { value: '/srv/new-project' } });
    await act(async () => { fireEvent.submit(input.closest('form')!); });
    expect(remote.request).toHaveBeenCalledWith(expect.objectContaining({ peerId: 'workstation', method: 'createSession' }));
    expect(local.api.createSession).not.toHaveBeenCalled(); expect(local.api.resolveDirectory).not.toHaveBeenCalled();
  });

  it('can choose Local while viewing a remote session and keeps its native picker', async () => {
    const { local, remote, user } = await setup();
    await user.click(screen.getByRole('button', { name: 'Select session Remote session' }));
    await user.click(screen.getByRole('button', { name: 'New session' })); await user.click(screen.getByRole('menuitem', { name: 'Local' }));
    expect(await screen.findByRole('button', { name: 'Browse folders' })).toBeVisible();
    expect(local.api.getHomeDirectory).toHaveBeenCalled();
    expect(remote.request.mock.calls.some(([input]) => input.method === 'getHomeDirectory')).toBe(false);
  });

  it('keeps connection icons inside the machine heading on connected and offline sections', async () => {
    await setup();
    for (const label of ['Disconnect Workstation', 'Connect Linux server']) {
      const button = screen.getByRole('button', { name: label });
      expect(button.closest('.machine-section-heading')).not.toBeNull(); expect(button.textContent).toBe('');
    }
    expect(document.querySelector('.machine-offline')).toBeNull();
  });

  it('notifies about incoming requests and opens the Remote access tab from Review', async () => {
    const { remote, user } = await setup();
    await act(async () => { remote.emit({ type: 'state', state: { ...remote.state, pending: [{ id: 'new-client', hostname: 'Build box', addresses: ['100.64.0.3'], online: true }] } }); });
    expect(await screen.findByText('Build box wants to connect')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Settings' })).toHaveAttribute('data-tip', 'Settings · 1 access request');
    await user.click(screen.getByRole('button', { name: 'Review' }));
    expect(screen.getByRole('tab', { name: 'Remote access' })).toHaveAttribute('aria-selected', 'true');
    await user.click(screen.getByRole('button', { name: 'Reject Build box' }));
    expect(remote.api.reject).toHaveBeenCalledWith({ peerId: 'new-client' });
  });

  it('resolves and completes folders remotely without a local picker', async () => {
    const { local, remote, user } = await setup();
    await user.click(screen.getByRole('button', { name: 'Select session Remote session' }));
    await user.click(screen.getByRole('button', { name: 'New session' }));
    await user.click(screen.getByRole('menuitem', { name: /Workstation/ }));
    expect(screen.queryByRole('button', { name: 'Browse folders' })).not.toBeInTheDocument();
    await waitFor(() => expect(remote.request).toHaveBeenCalledWith({ peerId: 'workstation', method: 'getHomeDirectory', payload: {} }));
    remote.request.mockResolvedValueOnce(success({ matches: ['/srv/project/'] }));
    const input = screen.getByRole('combobox', { name: 'Folder path' });
    fireEvent.change(input, { target: { value: '/srv/pr' } });
    await act(async () => { fireEvent.keyDown(input, { key: 'Tab' }); });
    expect(input).toHaveValue('/srv/project/');
    remote.request.mockResolvedValueOnce(success({ cwd: '/srv/project' })).mockResolvedValueOnce(success(remote.remoteSnapshot.sessions[0]!));
    await act(async () => { fireEvent.submit(input.closest('form')!); });
    expect(remote.request).toHaveBeenCalledWith(expect.objectContaining({ peerId: 'workstation', method: 'createSession', payload: expect.objectContaining({ cwd: '/srv/project' }) }));
    expect(local.api.chooseDirectory).not.toHaveBeenCalled(); expect(local.api.resolveDirectory).not.toHaveBeenCalled(); expect(local.api.completeDirectory).not.toHaveBeenCalled(); expect(local.api.createSession).not.toHaveBeenCalled();
  });

  it('disconnects and closes the client without closing any remote PTY or falling back to local actions', async () => {
    const { local, remote, user, unmount } = await setup();
    await user.click(screen.getByRole('button', { name: 'Select session Remote session' }));
    await user.click(screen.getByRole('button', { name: 'Disconnect Workstation' }));
    expect(remote.api.disconnect).toHaveBeenCalledWith({ peerId: 'workstation' });
    expect(await screen.findByRole('heading', { name: 'Machine disconnected' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'New session' })).toBeEnabled();
    expect(local.api.createSession).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'Select session Session 1' })).toHaveAttribute('aria-pressed', 'false');
    unmount();
    expect(remote.request.mock.calls.some(([input]) => input.method === 'closeTab' || input.method === 'deleteSession')).toBe(false);
    expect(local.api.closeTab).not.toHaveBeenCalled(); expect(remote.listeners.size).toBe(0);
  });

  it('keeps appearance and settings local while a remote machine is selected', async () => {
    const { local, remote, user } = await setup();
    await user.click(screen.getByRole('button', { name: 'Select session Remote session' }));
    await user.click(screen.getByRole('button', { name: 'Settings' }));
    openSettingsTab('Remote access');
    expect(screen.getByRole('switch', { name: 'Allow remote access' })).toBeVisible();
    openSettingsTab('General');
    const swatch = screen.getByRole('button', { name: 'Use #a78bfa' });
    await user.click(swatch);
    await waitFor(() => expect(local.api.saveSettings).toHaveBeenCalled());
    expect(remote.request.mock.calls.some(([input]) => String(input.method) === 'saveSettings')).toBe(false);
  });
});
