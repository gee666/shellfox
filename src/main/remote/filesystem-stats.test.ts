import path from 'node:path';
import { stat } from 'node:fs/promises';
import { afterEach, expect, it, vi } from 'vitest';
import { success } from '../../shared/contracts';
import { FakeRemoteHelper, remoteFixture } from './test-fixtures';

const cleanups: Array<() => Promise<void>> = [];
afterEach(async () => { for (const close of cleanups.splice(0).reverse()) await close(); });
it('requires approval for mkdir/stats, creates only after approval, and strips stats secrets and elevated processes', async () => {
  const network = new Map<string, FakeRemoteHelper>();
  const host = await remoteFixture(network, 'host'), client = await remoteFixture(network, 'client'); cleanups.push(host.dispose, client.dispose);
  const created = await host.create(); if (!created.ok) throw new Error('Create failed');
  await host.service.setSessionEnv({ sessionId: created.value.id, env: [{ name: 'SECRET', value: 'never-in-metrics' }] });
  const cwd = path.join(host.directory, 'new', 'nested'); await client.remote.connect({ peerId: 'host' });
  await vi.waitFor(() => expect(client.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ connection: 'awaiting-approval' })] } }));
  for (const request of [{ method: 'createDirectory', payload: { path: cwd } }, { method: 'getSessionStats', payload: {} }]) expect(await client.remote.request({ peerId: 'host', ...request })).toMatchObject({ ok: false, error: { code: 'APPROVAL_REQUIRED' } });
  await expect(stat(cwd)).rejects.toMatchObject({ code: 'ENOENT' });
  await host.remote.approve({ peerId: 'client' });
  // Approval reconnect runs on a one-second timer; the default one-second wait races it.
  await vi.waitFor(() => expect(client.remote.getState()).toMatchObject({ ok: true, value: { peers: [expect.objectContaining({ connection: 'connected' })] } }), { timeout: 5000 });
  expect(await client.remote.request({ peerId: 'host', method: 'createDirectory', payload: { path: cwd } })).toEqual({ ok: true, value: { cwd } });
  expect((await stat(cwd)).isDirectory()).toBe(true);
  const snapshot = host.service.getSnapshot(); if (!snapshot.ok) throw new Error('Snapshot failed');
  const tabId = created.value.tabs[0].id;
  const metrics = { tabId, processes: [{ pid: 123, parentPid: 1, name: 'shell', memoryBytes: 42, elapsedSeconds: 2, domain: 'local' }], memoryBytes: 42, elapsedSeconds: 2 };
  const stats = vi.spyOn(host.service, 'getSessionStats').mockResolvedValue(success({ sessions: snapshot.value.sessions, tabs: [metrics] }));
  const result = await client.remote.request({ peerId: 'host', method: 'getSessionStats', payload: { sessionId: created.value.id } });
  expect(result).toMatchObject({ ok: true, value: { sessions: [{ env: [] }], tabs: [metrics] } }); expect(JSON.stringify(result)).not.toContain('never-in-metrics');
  const tab = host.repository.tab(tabId); if (!tab?.terminal) throw new Error('Tab missing'); tab.terminal.profileId = 'administrator-powershell'; host.repository.saveTab(tab);
  const profiles = host.service.getTerminalProfiles(); if (!profiles.ok) throw new Error('Profiles missing');
  vi.spyOn(host.service, 'getTerminalProfiles').mockReturnValue(success({ ...profiles.value, profiles: [...profiles.value.profiles, { ...profiles.value.profiles[0], id: 'administrator-powershell', elevation: 'administrator' }] }));
  const hidden = host.service.getSnapshot(); if (!hidden.ok) throw new Error('Snapshot failed'); stats.mockResolvedValue(success({ sessions: hidden.value.sessions, tabs: [metrics] }));
  expect(await client.remote.request({ peerId: 'host', method: 'getSessionStats', payload: {} })).toMatchObject({ ok: true, value: { sessions: [{ tabs: [], env: [] }], tabs: [] } });
});
