import { randomUUID } from 'node:crypto';
import { expect, it, vi } from 'vitest';
import { PtyBackend } from './backend';
import { EmbeddedSessionService } from './service';
import { FakePty, factoryFixture, MemoryRepository } from './test-fixtures';
import { ADMIN_POWERSHELL_PROFILE } from '../../shared/contracts';
import type { Result } from '../../shared/contracts';
const value = <T>(result: Result<T>): T => { if (!result.ok) throw new Error(result.error.message); return result.value; };
async function fixture(directoryWait = false) {
  const f = factoryFixture();
  const regular = { id: 'windows-powershell', label: 'Windows PowerShell', environment: 'local' as const, executable: String.raw`C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe`, args: ['-NoLogo'], distro: null, available: true };
  const admin = { ...regular, id: ADMIN_POWERSHELL_PROFILE, label: 'Administrator Windows PowerShell', elevation: 'administrator' as const };
  let approve!: () => void, directory!: () => void;
  const ready = new Promise<void>(resolve => { approve = resolve; });
  const cwd = new Promise<string>(resolve => { directory = () => resolve(String.raw`C:\work`); });
  const elevated = Object.assign(new FakePty(), { birth: '133000000000000009', whenReady: vi.fn(() => ready), closeConfirmed: vi.fn(async () => { elevated.exit(1, 0); }) });
  const elevate = vi.fn(() => elevated), validate = vi.fn((profile: { elevation?: string }, path: string) => directoryWait && profile.elevation ? cwd : Promise.resolve(path));
  const backend = new PtyBackend({ ...f.options, validateCwd: validate, discover: async () => ({ profiles: [regular, admin], defaultProfileId: regular.id, lifetime: 'app-owned', shellSurvival: false }), elevatedFactory: elevate });
  const repository = new MemoryRepository(), service = new EmbeddedSessionService(repository, backend, () => ({ setWatch: vi.fn(), dispose: vi.fn() }));
  await service.initialize();
  const session = value(await service.createSession({ cwd: String.raw`C:\work`, requestId: randomUUID() }));
  const adding = service.addTab({ sessionId: session.id, profileId: admin.id });
  await vi.waitFor(() => expect(directoryWait ? validate : elevated.whenReady).toHaveBeenCalledTimes(directoryWait ? 2 : 1));
  const tab = repository.tabs(session.id).at(-1)!;
  return { ...f, backend, service, repository, session, adding, tab, elevated, elevate, approve, directory };
}
it('dispose cancels pending UAC before draining the same operation queue, and ignores delayed approval', async () => {
  const f = await fixture();
  const queued = f.service.addTab({ sessionId: f.session.id });
  await f.service.dispose();
  expect(f.elevated.closeConfirmed).toHaveBeenCalledOnce();
  expect(await f.adding).toMatchObject({ ok: false, error: { code: 'LAUNCH_FAILED' } });
  expect(await queued).toMatchObject({ ok: false, error: { code: 'NATIVE_UNAVAILABLE' } });
  expect(f.repository.tab(f.tab.id)?.lifecycle).toBe('closed');
  f.approve(); await Promise.resolve();
  expect(f.backend.live()).toHaveLength(0); expect(f.factory).toHaveBeenCalledTimes(1); expect(f.elevate).toHaveBeenCalledOnce();
});
it('update shutdown cancels pending UAC before waiting for its queue and can resume normal tabs', async () => {
  const f = await fixture();
  const resume = await f.service.closeForUpdate();
  expect(f.elevated.closeConfirmed).toHaveBeenCalledOnce();
  expect(await f.adding).toMatchObject({ ok: false, error: { code: 'LAUNCH_FAILED' } });
  f.approve(); await Promise.resolve(); expect(f.backend.live()).toHaveLength(0);
  resume(); value(await f.service.addTab({ sessionId: f.session.id }));
  expect(f.elevate).toHaveBeenCalledOnce(); await f.service.dispose();
});
it('same-session close interrupts the deferred startup rather than queuing behind UAC', async () => {
  const f = await fixture();
  expect(await f.service.closeTab({ tabId: f.tab.id, generation: randomUUID() })).toMatchObject({ ok: false, error: { code: 'TARGET_LOST' } });
  expect(f.elevated.closeConfirmed).not.toHaveBeenCalled();
  value(await f.service.closeTab({ tabId: f.tab.id, generation: f.tab.operationId }));
  expect(await f.adding).toMatchObject({ ok: false, error: { code: 'LAUNCH_FAILED' } });
  expect(f.elevated.closeConfirmed).toHaveBeenCalledOnce();
  f.approve(); await Promise.resolve();
  expect(f.repository.tab(f.tab.id)?.lifecycle).toBe('closed');
  value(await f.service.addTab({ sessionId: f.session.id })); expect(f.elevate).toHaveBeenCalledOnce();
  await f.service.dispose();
});
it('close cancels an intent still validating cwd, before any helper exists', async () => {
  const f = await fixture(true);
  value(await f.service.closeTab({ tabId: f.tab.id, generation: f.tab.operationId }));
  expect(await f.adding).toMatchObject({ ok: false, error: { code: 'LAUNCH_FAILED' } });
  f.directory(); await Promise.resolve(); expect(f.elevate).not.toHaveBeenCalled();
  expect(f.repository.tab(f.tab.id)?.lifecycle).toBe('closed'); await f.service.dispose();
});
it('shutdown cancels cwd validation before any helper exists, even if validation finishes after shutdown', async () => {
  const f = await fixture(true);
  await f.service.dispose(); expect(await f.adding).toMatchObject({ ok: false, error: { code: 'LAUNCH_FAILED' } });
  f.directory(); await Promise.resolve(); expect(f.elevate).not.toHaveBeenCalled(); expect(f.backend.live()).toHaveLength(0);
});
it('shutdown errors keep the unconfirmed helper owned even after the operation queue is interrupted', async () => {
  const f = await fixture(); f.elevated.closeConfirmed.mockRejectedValue(new Error('No cleanup reply.'));
  await expect(f.service.closeForUpdate()).rejects.toThrow('shutdown was not confirmed');
  expect(await f.adding).toMatchObject({ ok: false }); expect(f.backend.live()).toHaveLength(1);
  expect(f.backend.get(f.tab.id)?.cleanupPending).toBe(true);
  f.elevated.closeConfirmed.mockImplementation(async () => { f.elevated.exit(1, 0); });
  await f.service.dispose();
});
