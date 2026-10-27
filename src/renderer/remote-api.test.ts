import { describe, expect, it, vi } from 'vitest';
import { failure, success } from '../shared/contracts';
import { createRemoteManagerApi } from './remote-api';
import { mockApi, snapshot } from './test-fixtures';
import { remoteFixture } from './remote-test-fixtures';

describe('machine API boundary', () => {
  it('routes session, terminal and directory requests to the chosen peer', async () => {
    const remote = remoteFixture(), local = mockApi().api;
    const api = createRemoteManagerApi(remote.api, 'workstation', local);
    remote.request.mockReset().mockResolvedValue(failure('UNSUPPORTED', 'Unavailable.'));
    await api.createSession({ cwd: '/srv/project', requestId: 'request-1' });
    await api.writeTerminal!({ tabId: 'same-tab', generation: 'generation', controlEpoch: 1, data: 'pwd\r' });
    await api.completeDirectory({ path: '/srv/pr' });
    await api.setSessionEnv({ sessionId: 'same-session', env: [{ name: 'Path', value: 'one' }, { name: 'PATH', value: 'two' }] });
    expect(remote.request.mock.calls.map(([input]) => input)).toEqual([
      { peerId: 'workstation', method: 'createSession', payload: { cwd: '/srv/project', requestId: 'request-1' } },
      { peerId: 'workstation', method: 'writeTerminal', payload: { tabId: 'same-tab', generation: 'generation', controlEpoch: 1, data: 'pwd\r' } },
      { peerId: 'workstation', method: 'completeDirectory', payload: { path: '/srv/pr' } },
      { peerId: 'workstation', method: 'setSessionEnv', payload: { sessionId: 'same-session', env: [{ name: 'Path', value: 'one' }, { name: 'PATH', value: 'two' }] } },
    ]);
    expect(local.createSession).not.toHaveBeenCalled(); expect(local.writeTerminal).not.toHaveBeenCalled();
  });

  it('keeps clipboard and browser operations local and blocks machine settings, pickers and integrations', async () => {
    const remote = remoteFixture(), local = mockApi().api;
    const openExternal = vi.fn(async () => success({ opened: true as const }));
    const api = createRemoteManagerApi(remote.api, 'server', { ...local, openExternal });
    await api.copyText({ text: '/srv/project' }); await api.readClipboardText!();
    await api.openExternal!({ url: 'https://example.com' });
    const operations = [api.saveSettings(snapshot().settings), api.chooseDirectory(), api.listSshProfiles(), api.setCliIntegration({ installed: true }), api.setExplorerIntegration({ installed: true }), api.openSessionFolder({ sessionId: 'one' })];
    expect((await Promise.all(operations)).every(result => !result.ok && result.error.code === 'UNSUPPORTED')).toBe(true);
    expect(local.copyText).toHaveBeenCalledWith({ text: '/srv/project' });
    expect(local.readClipboardText).toHaveBeenCalledOnce(); expect(openExternal).toHaveBeenCalledWith({ url: 'https://example.com' });
    expect(remote.request).not.toHaveBeenCalled(); expect(local.saveSettings).not.toHaveBeenCalled(); expect(local.chooseDirectory).not.toHaveBeenCalled();
  });

  it('filters same-ID terminal and session events by peer and unsubscribes cleanly', () => {
    const remote = remoteFixture(), local = mockApi().api;
    const api = createRemoteManagerApi(remote.api, 'workstation', local), changed = vi.fn(), terminal = vi.fn();
    const stopChanged = api.subscribe(changed), stopTerminal = api.subscribeTerminal!(terminal);
    const event = { type: 'data' as const, controlEpoch: 1, tabId: 'tab-1', generation: 'g', sequence: 1, data: 'remote' };
    remote.emit({ type: 'terminal', peerId: 'server', event }); remote.emit({ type: 'changed', peerId: 'server', event: { revision: 1, reason: 'sessions' } });
    expect(terminal).not.toHaveBeenCalled(); expect(changed).not.toHaveBeenCalled();
    remote.emit({ type: 'terminal', peerId: 'workstation', event }); remote.emit({ type: 'changed', peerId: 'workstation', event: { revision: 2, reason: 'sessions' } });
    expect(terminal).toHaveBeenCalledExactlyOnceWith(event); expect(changed).toHaveBeenCalledOnce();
    stopChanged(); stopTerminal(); expect(remote.listeners.size).toBe(0);
  });
});
