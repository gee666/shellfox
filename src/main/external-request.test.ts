import { expect, it, vi } from 'vitest';
import { randomUUID } from 'node:crypto';
import { EarlyRequestQueue, handleCliRequest, parseCli } from './cli';
import { EmbeddedSessionService } from './terminal/service';
import { PtyBackend } from './terminal/backend';
import { MemoryRepository, factoryFixture } from './terminal/test-fixtures';
import type { ChangedEvent, ExplorerIntegrationDto } from '../shared/contracts';
import { failure, success } from '../shared/contracts';

const windowFixture = () => ({ isDestroyed: () => false, isMinimized: () => true, restore: vi.fn(), show: vi.fn(), focus: vi.fn() });
it('routes cold-start/forwarded Explorer argv through the queue to one selected embedded default-profile tab', async () => {
  const f = factoryFixture(), repository = new MemoryRepository();
  const service = new EmbeddedSessionService(repository, new PtyBackend(f.options), () => ({ setWatch() {}, dispose() {} }));
  await service.initialize(); const events: ChangedEvent[] = []; service.subscribe(e => events.push(e));
  const parsed = parseCli(['--new-session', '--cwd', 'C:\\Projects\\some folder\\.']); if (!parsed.ok) throw new Error('parse');
  const queue = new EarlyRequestQueue(), window = windowFixture();
  queue.enqueue(parsed.value.request); queue.enqueue(parsed.value.request);
  expect(repository.sessions()).toHaveLength(0);
  queue.ready(async request => { expect(await handleCliRequest(request, service, window)).toEqual(success({ handled: true })); });
  await queue.idle();
  const snapshot = service.getSnapshot(); if (!snapshot.ok) throw new Error('snapshot');
  const session = snapshot.value.sessions[0];
  expect(session).toMatchObject({ title: 'some folder', cwd: 'C:\\Projects\\some folder', adapterId: 'embedded-pty', tabs: [{ lifecycle: 'open', terminalKind: 'embedded', profileId: 'login-shell' }] });
  expect(f.factory).toHaveBeenCalledTimes(1);
  expect(events.filter(e => e.selectSessionId)).toEqual([{ revision: service.revision, reason: 'sessions', selectSessionId: session.id }]);
  expect(window.restore).toHaveBeenCalledOnce(); expect(window.show).toHaveBeenCalledOnce(); expect(window.focus).toHaveBeenCalledOnce();
  // A fresh invocation for the same directory creates a separate session.
  queue.enqueue({ ...parsed.value.request, requestId: randomUUID() }); await queue.idle();
  expect(repository.sessions()).toHaveLength(2); expect(f.factory).toHaveBeenCalledTimes(2);
  await service.dispose();
});
it('handles background test requests without showing, restoring or focusing the window', async () => {
  const window = windowFixture(), host = { createSession: vi.fn(), selectSession: vi.fn() };
  expect(await handleCliRequest({ version: 1, kind: 'show' }, host, window, false)).toEqual(success({ handled: true }));
  expect(window.show).not.toHaveBeenCalled(); expect(window.restore).not.toHaveBeenCalled(); expect(window.focus).not.toHaveBeenCalled();
});
it('keeps POSIX CLI paths in POSIX syntax even when reviewing on Windows', async () => {
  const f = factoryFixture(), repository = new MemoryRepository();
  const service = new EmbeddedSessionService(repository, new PtyBackend(f.options), () => ({ setWatch() {}, dispose() {} }));
  await service.initialize();
  const result = await handleCliRequest({ version: 1, kind: 'new-session', cwd: '/tmp/some folder/./child/..', requestId: randomUUID() }, service, windowFixture());
  expect(result).toEqual(success({ handled: true }));
  expect(repository.sessions()[0]).toMatchObject({ cwd: '/tmp/some folder', title: 'some folder' });
  await service.dispose();
});
it('does not select a failed creation or open another legacy tab', async () => {
  const window = windowFixture(), host = { createSession: vi.fn(async () => failure('VALIDATION', 'Not a directory')), selectSession: vi.fn() };
  const result = await handleCliRequest({ version: 1, kind: 'new-session', cwd: 'C:\\missing', requestId: randomUUID() }, host, window);
  expect(result.ok).toBe(false); expect(host.selectSession).not.toHaveBeenCalled();
  expect(await handleCliRequest({ version: 1, kind: 'show' }, host, window)).toEqual(success({ handled: true }));
  expect(host.createSession).toHaveBeenCalledTimes(1); expect(window.focus).toHaveBeenCalledOnce();
});
it('exposes embedded Explorer support and saves the opt-in only after registry success', async () => {
  const f = factoryFixture(), repository = new MemoryRepository();
  const state: ExplorerIntegrationDto = { supported: true, installed: false, folderItemInstalled: false, backgroundInstalled: false, reason: null };
  const integration = { get: vi.fn(async () => success(state)), set: vi.fn(async (installed: boolean) => success({ ...state, installed, folderItemInstalled: installed, backgroundInstalled: installed })) };
  const service = new EmbeddedSessionService(repository, new PtyBackend(f.options), () => ({ setWatch() {}, dispose() {} }), integration);
  await service.initialize(); expect(service.explorer.supported).toBe(true); expect(service.probe.capabilities.explorerContextMenu).toBe(true);
  expect(await service.setExplorerIntegration({ installed: true })).toMatchObject({ ok: true, value: { installed: true } });
  expect(repository.explorerPreference()).toBe(true);
  integration.set.mockImplementationOnce(async () => failure('AUTH_FAILED', 'Foreign key'));
  expect((await service.setExplorerIntegration({ installed: false })).ok).toBe(false); expect(repository.explorerPreference()).toBe(true);
  await service.setExplorerIntegration({ installed: false }); expect(repository.explorerPreference()).toBe(false);
  expect((await service.setExplorerIntegration({ installed: true, command: 'evil' } as { installed: boolean })).ok).toBe(false);
  expect(integration.set).toHaveBeenCalledTimes(3); await service.dispose();
});
