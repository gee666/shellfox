// @vitest-environment jsdom
import './terminal-test-mocks';
import { afterEach, expect, it, vi } from 'vitest';
import { success } from '../shared/contracts';
import type { RemoteMethod, RemoteResponse } from '../shared/remote-contracts';
import { responseSchemas } from '../shared/schemas';
import { createRemoteController, type RemoteController } from './remote-controller';
import { remoteFixture } from './remote-test-fixtures';
import { deferred, mockApi, profiles, session } from './test-fixtures';
import { visibilityReporter } from './visibility-reporter';

const controllers: RemoteController[] = [];
const flush = async () => { for (let i = 0; i < 40; i++) await Promise.resolve(); };
afterEach(() => { controllers.splice(0).forEach(controller => controller.stop()); vi.useRealTimers(); });

it('bounds desktop metadata pressure across 1500 duplicate native/state events and slow visibility changes', async () => {
  vi.useFakeTimers();
  const remote = remoteFixture();
  type Reply = RemoteResponse<RemoteMethod>;
  const work: { method: RemoteMethod; payload: unknown; resolve: (reply: Reply) => void }[] = [];
  let inFlight = 0, peak = 0;
  remote.request.mockReset().mockImplementation(({ method, payload }) => {
    const pending = deferred<Reply>();
    work.push({ method, payload, resolve: pending.resolve });
    peak = Math.max(peak, ++inFlight);
    return pending.promise.finally(() => { inFlight--; });
  });
  const controller = createRemoteController(remote.api, mockApi().api); controllers.push(controller); controller.start();
  await flush();
  const runtime = controller.store.getState().runtimes.workstation;
  if (!runtime) throw new Error('Missing remote runtime');
  const report = visibilityReporter(runtime.client.api);
  report([]);
  for (let i = 0; i < 1500; i++) {
    const event = { type: 'changed' as const, peerId: 'workstation', event: { revision: i + 2, reason: 'native' as const } };
    remote.emit(event); remote.emit(event); // Base connection and local RPC view may both forward it.
    remote.emit({ type: 'state', state: remote.state });
    report([session(i + 1).id]);
  }
  await vi.advanceTimersByTimeAsync(60_000);
  expect(work.map(item => item.method).sort()).toEqual(['getSnapshot', 'getTerminalProfiles', 'setVisibleSessions']);
  expect(peak).toBe(3); expect(inFlight).toBe(3);
  const complete = (method: RemoteMethod, reply: Reply) => {
    const pending = work.find(item => item.method === method);
    if (!pending) throw new Error(`Missing ${method}`);
    pending.resolve(reply);
  };
  complete('getSnapshot', responseSchemas.getSnapshot.parse(success({ ...remote.remoteSnapshot, revision: 1501 })));
  complete('getTerminalProfiles', responseSchemas.getTerminalProfiles.parse(success({ profiles, defaultProfileId: 'pwsh', lifetime: 'app-owned', shellSurvival: false })));
  complete('setVisibleSessions', success({ configured: true }));
  await flush();
  expect(work.map(item => item.method)).toEqual(['getSnapshot', 'getTerminalProfiles', 'setVisibleSessions', 'setVisibleSessions']);
  expect(work[3]?.payload).toEqual({ sessionIds: [session(1500).id] });
  work[3]?.resolve(success({ configured: true })); await flush();
  expect(runtime.client.store.getState().snapshot?.revision).toBe(1501);
  for (let i = 0; i < 1500; i++) {
    remote.emit({ type: 'changed', peerId: 'workstation', event: { revision: 1501, reason: 'native' } });
    remote.emit({ type: 'state', state: remote.state });
  }
  await vi.advanceTimersByTimeAsync(60_000);
  expect(work).toHaveLength(4); expect(inFlight).toBe(0); expect(peak).toBe(3);
});

it('shares a single visibility lane across cleanup/remount and delivers only the latest set', async () => {
  vi.useFakeTimers();
  const f = mockApi(), pending = deferred<Awaited<ReturnType<typeof f.api.setVisibleSessions>>>();
  f.api.setVisibleSessions.mockReturnValueOnce(pending.promise);
  const old = visibilityReporter(f.api), remounted = visibilityReporter(f.api);
  expect(remounted).toBe(old);
  old([session(1).id]); old([]); remounted([]);
  for (let i = 0; i < 1500; i++) remounted([session(i + 2).id]);
  await vi.advanceTimersByTimeAsync(60_000);
  expect(f.api.setVisibleSessions).toHaveBeenCalledTimes(1);
  pending.resolve(success({ configured: true })); await flush();
  expect(f.api.setVisibleSessions).toHaveBeenCalledTimes(2);
  expect(f.api.setVisibleSessions).toHaveBeenLastCalledWith({ sessionIds: [session(1501).id] });
  remounted([]); await flush();
  expect(f.api.setVisibleSessions).toHaveBeenCalledTimes(3);
  expect(f.api.setVisibleSessions).toHaveBeenLastCalledWith({ sessionIds: [] });
});
