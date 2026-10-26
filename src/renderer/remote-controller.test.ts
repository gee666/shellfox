// @vitest-environment jsdom
import './terminal-test-mocks';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { failure, success } from '../shared/contracts';
import type { RemoteResponse } from '../shared/remote-contracts';
import { responseSchemas } from '../shared/schemas';
import { createRemoteController, type RemoteController } from './remote-controller';
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

  it('applies a slow successful response despite repeated state and native events, then refreshes once', async () => {
    const f = setup(), pending = deferred<RemoteResponse<'getTerminalProfiles'>>();
    f.getProfiles.mockReturnValueOnce(pending.promise);
    await vi.waitFor(() => expect(f.getProfiles).toHaveBeenCalledTimes(1));
    vi.useFakeTimers();
    for (let index = 0; index < 20; index++) {
      f.connected();
      f.remote.emit({ type: 'changed', peerId: 'workstation', event: { revision: 1, reason: 'native' } });
    }
    pending.resolve(f.profileResult);
    await vi.advanceTimersByTimeAsync(0);
    expect(f.runtime()?.profiles).toHaveLength(profiles.length);
    expect(f.getProfiles).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(5_000);
    expect(f.getProfiles).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(f.getProfiles).toHaveBeenCalledTimes(2);
  });

  it('refreshes changed availability after queued host events without overlapping requests', async () => {
    const f = setup(); await ready(f); vi.useFakeTimers();
    const pending = deferred<RemoteResponse<'getTerminalProfiles'>>();
    f.getProfiles.mockReturnValueOnce(pending.promise);
    const changed = () => f.remote.emit({ type: 'changed', peerId: 'workstation', event: { revision: 1, reason: 'native' } });
    changed(); await vi.advanceTimersByTimeAsync(5_000);
    expect(f.getProfiles).toHaveBeenCalledTimes(2);
    for (let index = 0; index < 20; index++) changed();
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
    f.connected(); await vi.advanceTimersByTimeAsync(5_000);
    f.disconnected(); f.connected(); await vi.advanceTimersByTimeAsync(0);
    expect(f.getProfiles).toHaveBeenCalledTimes(3);
    pending.resolve(responseSchemas.getTerminalProfiles.parse(success({ profiles: [], defaultProfileId: null, lifetime: 'app-owned', shellSurvival: false })));
    await vi.advanceTimersByTimeAsync(10_000);
    expect(f.runtime()?.profiles).toHaveLength(profiles.length);
    expect(f.getProfiles).toHaveBeenCalledTimes(3);
  });
});
