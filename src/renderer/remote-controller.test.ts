// @vitest-environment jsdom
import './terminal-test-mocks';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { failure, success } from '../shared/contracts';
import type { RemoteResponse, RemoteState } from '../shared/remote-contracts';
import { responseSchemas } from '../shared/schemas';
import { createRemoteController, type RemoteController } from './remote-controller';
import { machineState } from './machine-state';
import { remoteFixture } from './remote-test-fixtures';
import { deferred, mockApi, profiles, session, snapshot } from './test-fixtures';

const controllers: RemoteController[] = [];
afterEach(() => { controllers.splice(0).forEach(controller => controller.stop()); vi.useRealTimers(); });
function setup() {
  const remote = remoteFixture();
  let current = remote.remoteSnapshot;
  const profileResult = responseSchemas.getTerminalProfiles.parse(success({ profiles, defaultProfileId: 'pwsh', lifetime: 'app-owned', shellSurvival: false }));
  const getProfiles = vi.fn<() => Promise<RemoteResponse<'getTerminalProfiles'>>>(async () => profileResult);
  remote.request.mockReset().mockImplementation(async ({ method }) => {
    if (method === 'getSnapshot') return responseSchemas.getSnapshot.parse(success(current));
    if (method === 'getTerminalProfiles') return getProfiles();
    return failure('UNSUPPORTED', 'Unavailable.');
  });
  const controller = createRemoteController(remote.api, mockApi().api);
  controllers.push(controller);
  const runtime = () => controller.store.getState().runtimes.workstation;
  const connected = () => remote.emit({ type: 'state', state: remote.state });
  const disconnected = () => remote.emit({ type: 'state', state: remote.disconnected });
  controller.start();
  return { remote, controller, getProfiles, profileResult, runtime, connected, disconnected, setSnapshot: (next: typeof current) => { current = next; } };
}
async function ready(f: ReturnType<typeof setup>) {
  await vi.waitFor(() => expect(f.runtime()?.client.store.getState().snapshot?.revision).toBe(1));
  await vi.waitFor(() => expect(f.runtime()?.profiles).toHaveLength(profiles.length));
}

it('keeps automatic unsupported-backend discovery quiet without hiding manual failures', async () => {
  const remote = remoteFixture();
  vi.spyOn(remote.api, 'getState').mockResolvedValue(failure('UNSUPPORTED', 'Remote access requires embedded terminals.'));
  const controller = createRemoteController(remote.api, mockApi().api); controllers.push(controller);
  controller.start();
  await vi.waitFor(() => expect(controller.store.getState().supported).toBe(false));
  expect(controller.store.getState().error).toBeNull();
  await controller.run(async () => failure('UNSUPPORTED', 'Manual operation unavailable.'));
  expect(controller.store.getState().error?.message).toBe('Manual operation unavailable.');
});

it('does not let a delayed action refresh overwrite a newer approval event', async () => {
  const f = setup(); await ready(f);
  const connect = deferred<Awaited<ReturnType<typeof f.remote.api.connect>>>();
  const fresh = deferred<Awaited<ReturnType<typeof f.remote.api.getState>>>();
  vi.mocked(f.remote.api.getState).mockReturnValueOnce(fresh.promise);
  const action = f.controller.run(() => connect.promise, { peerAction: { peerId: 'server', kind: 'connect' } });
  f.remote.emit({ type: 'state', state: { ...f.remote.state, pending: [{ id: 'unrelated', hostname: 'Unrelated device', online: true, addresses: [] }] } });
  connect.resolve(success({ ...f.remote.state, peers: f.remote.state.peers.map(peer => peer.id === 'server' ? { ...peer, connection: 'connecting' } : peer) }));
  await vi.waitFor(() => expect(f.remote.api.getState).toHaveBeenCalledTimes(2));
  const latest = { ...f.remote.state, peers: f.remote.state.peers.map(peer => peer.id === 'server' ? { ...peer, connection: 'rejected' as const } : peer) };
  f.remote.emit({ type: 'state', state: latest });
  fresh.resolve(success(f.remote.state)); await action;
  expect(f.controller.store.getState().state?.peers.find(peer => peer.id === 'server')?.connection).toBe('rejected');
});

it('fences pending peer actions across controller stop and restart', async () => {
  const f = setup(); await ready(f);
  const pending = deferred<Awaited<ReturnType<typeof f.remote.api.connect>>>();
  const action = f.controller.run(() => pending.promise, { peerAction: { peerId: 'server', kind: 'connect' } });
  f.controller.stop(); f.controller.start(); await ready(f);
  pending.resolve(failure('NATIVE_UNAVAILABLE', 'Retired action.')); await action;
  expect(f.controller.store.getState().error).toBeNull();
  expect(f.controller.store.getState().state?.peers.find(peer => peer.id === 'server')?.connection).toBe('disconnected');
});

it('keeps an authenticated connected runtime and its label through control-health error/starting with stale offline discovery', async () => {
  const f = setup(); await ready(f);
  const runtime = f.runtime()!;
  const dispose = vi.spyOn(runtime.registry, 'dispose');
  for (const status of ['error', 'starting'] as const) {
    const state: RemoteState = { ...f.remote.state, status, error: 'Control connection is recovering.', peers: f.remote.state.peers.map(peer => ({ ...peer, online: false })) };
    f.remote.emit({ type: 'state', state });
    expect(f.runtime()).toBe(runtime);
    expect(machineState(state.peers[0]!)).toMatchObject({ label: 'Connected', active: true });
    expect((await runtime.client.api.getSnapshot()).ok).toBe(true);
  }
  expect(dispose).not.toHaveBeenCalled();
  expect(f.remote.request.mock.calls.some(([input]) => input.method === 'detachTerminal' || input.method === 'closeTab')).toBe(false);
});

it('retires connected runtimes on actual HTTP failure, rejection, explicit disconnect, or global auth loss/disable', async () => {
  const cases: { status: RemoteState['status']; enabled: boolean; connection: RemoteState['peers'][number]['connection'] }[] = [
    { status: 'error', enabled: true, connection: 'reconnecting' },
    { status: 'online', enabled: true, connection: 'rejected' },
    { status: 'online', enabled: true, connection: 'disconnected' },
    { status: 'login-required', enabled: true, connection: 'connected' },
    { status: 'disabled', enabled: true, connection: 'connected' },
    { status: 'online', enabled: false, connection: 'connected' },
  ];
  for (const { connection, ...network } of cases) {
    const f = setup(); await ready(f);
    const dispose = vi.spyOn(f.runtime()!.registry, 'dispose');
    f.remote.emit({ type: 'state', state: { ...f.remote.state, ...network, peers: f.remote.state.peers.map(peer => ({ ...peer, online: false, connection })) } });
    expect(f.runtime()).toBeUndefined(); expect(dispose).toHaveBeenCalledOnce();
    expect(f.remote.request.mock.calls.some(([input]) => input.method === 'closeTab')).toBe(false);
  }
});

describe('remote reconnection', () => {
  it('restores the selected session and tab after reconnect without activating or launching shells', async () => {
    const f = setup(), first = session(1), second = session(2);
    second.tabs.push({ ...session(3).tabs[0]!, sessionId: second.id });
    f.setSnapshot(snapshot([first, second])); await ready(f);
    const original = f.runtime()!;
    original.client.selectTab(second, second.tabs[1]!.id);
    f.disconnected(); expect(f.runtime()).toBeUndefined();
    f.connected(); await ready(f);
    const restored = f.runtime()!;
    expect(restored.client).not.toBe(original.client);
    expect(restored.client.store.getState().selectedId).toBe(second.id);
    expect(restored.client.store.getState().activeTabIds[second.id]).toBe(second.tabs[1]!.id);
    expect(f.remote.request.mock.calls.every(([input]) => ['getSnapshot', 'getTerminalProfiles'].includes(input.method))).toBe(true);
  });

  it.each(['session', 'tab'] as const)('does not select another shell when the saved %s disappeared', async missing => {
    const f = setup(), first = session(1), second = session(2);
    second.tabs.push({ ...session(3).tabs[0]!, sessionId: second.id });
    f.setSnapshot(snapshot([first, second])); await ready(f);
    f.runtime()!.client.selectTab(second, second.tabs[1]!.id);
    f.disconnected();
    f.setSnapshot(snapshot(missing === 'session' ? [first] : [first, { ...second, tabs: [second.tabs[0]!] }]));
    f.connected(); await ready(f);
    expect(f.runtime()!.client.store.getState().selectedId).toBeNull();
    f.remote.emit({ type: 'changed', peerId: 'workstation', event: { revision: 1, reason: 'sessions' } });
    await Promise.resolve(); await Promise.resolve();
    expect(f.runtime()!.client.store.getState().selectedId).toBeNull();
  });
});

describe('remote shell discovery', () => {
  it('recovers an initial failure on a healthy state refresh, coalescing repeated refreshes', async () => {
    const f = setup();
    f.getProfiles.mockResolvedValueOnce(failure('NATIVE_UNAVAILABLE', 'Shell discovery failed.'));
    await vi.waitFor(() => expect(f.controller.store.getState().error?.message).toBe('Shell discovery failed.'));
    vi.useFakeTimers();
    for (let index = 0; index < 20; index++) f.connected();
    expect(f.getProfiles).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(5_000);
    expect(f.getProfiles).toHaveBeenCalledTimes(2);
    expect(f.runtime()?.profiles).toHaveLength(profiles.length);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(f.getProfiles).toHaveBeenCalledTimes(2);
  });

  it('keeps one slow profile read across 1500 poll-only native and repeated state events', async () => {
    const f = setup(), pending = deferred<RemoteResponse<'getTerminalProfiles'>>();
    f.getProfiles.mockReturnValueOnce(pending.promise);
    await vi.waitFor(() => expect(f.getProfiles).toHaveBeenCalledTimes(1));
    vi.useFakeTimers();
    for (let index = 0; index < 1500; index++) {
      f.connected();
      f.remote.emit({ type: 'changed', peerId: 'workstation', event: { revision: 1, reason: 'native' } });
    }
    await vi.advanceTimersByTimeAsync(60_000);
    expect(f.getProfiles).toHaveBeenCalledTimes(1);
    pending.resolve(f.profileResult);
    await vi.advanceTimersByTimeAsync(0);
    expect(f.runtime()?.profiles).toHaveLength(profiles.length);
    expect(f.getProfiles).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(5_000);
    expect(f.getProfiles).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(f.getProfiles).toHaveBeenCalledTimes(1);
  });

  it('refreshes changed availability after queued host events without overlapping requests', async () => {
    const f = setup(); await ready(f); vi.useFakeTimers();
    const pending = deferred<RemoteResponse<'getTerminalProfiles'>>();
    f.getProfiles.mockReturnValueOnce(pending.promise);
    const changed = (revision: number) => f.remote.emit({ type: 'changed', peerId: 'workstation', event: { revision, reason: 'native' } });
    f.setSnapshot({ ...f.remote.remoteSnapshot, revision: 2, probe: { ...f.remote.remoteSnapshot.probe, available: false } });
    changed(2); await vi.advanceTimersByTimeAsync(5_000);
    expect(f.getProfiles).toHaveBeenCalledTimes(2);
    f.setSnapshot({ ...f.remote.remoteSnapshot, revision: 3, settings: { ...f.remote.remoteSnapshot.settings, pythonPath: '/new/python' } });
    for (let index = 0; index < 20; index++) changed(3);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(f.getProfiles).toHaveBeenCalledTimes(2);
    const updated = responseSchemas.getTerminalProfiles.parse(success({ profiles: [{ ...profiles[0]!, available: false }], defaultProfileId: null, lifetime: 'app-owned', shellSurvival: false }));
    f.getProfiles.mockResolvedValueOnce(updated);
    pending.resolve(f.profileResult);
    await vi.advanceTimersByTimeAsync(0);
    expect(f.getProfiles).toHaveBeenCalledTimes(3);
    expect(f.runtime()?.profiles).toEqual([{ ...profiles[0]!, available: false }]);
    expect(f.runtime()?.defaultProfileId).toBeNull();
    expect(f.remote.request.mock.calls.filter(([input]) => input.method === 'getTerminalProfiles')).toHaveLength(3);
  });

  it('ignores replies and cancels scheduled refreshes belonging to a disconnected runtime', async () => {
    const f = setup(); await ready(f); vi.useFakeTimers();
    const pending = deferred<RemoteResponse<'getTerminalProfiles'>>();
    f.getProfiles.mockReturnValueOnce(pending.promise);
    f.setSnapshot({ ...f.remote.remoteSnapshot, revision: 2, probe: { ...f.remote.remoteSnapshot.probe, available: false } });
    f.remote.emit({ type: 'changed', peerId: 'workstation', event: { revision: 2, reason: 'native' } });
    await vi.advanceTimersByTimeAsync(5_000);
    f.disconnected(); f.connected(); await vi.advanceTimersByTimeAsync(0);
    expect(f.getProfiles).toHaveBeenCalledTimes(3);
    pending.resolve(responseSchemas.getTerminalProfiles.parse(success({ profiles: [], defaultProfileId: null, lifetime: 'app-owned', shellSurvival: false })));
    await vi.advanceTimersByTimeAsync(10_000);
    expect(f.runtime()?.profiles).toHaveLength(profiles.length);
    expect(f.getProfiles).toHaveBeenCalledTimes(3);
  });
});
