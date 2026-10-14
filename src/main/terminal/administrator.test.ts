import { randomUUID } from 'node:crypto';
import { expect, it, vi } from 'vitest';
import type { Result, TerminalProfileDto, TerminalProfilesDto } from '../../shared/contracts';
import { PtyBackend } from './backend';
import { EmbeddedSessionService } from './service';
import { ADMIN_POWERSHELL_PROFILE } from './windows-elevated';
import { FakePty, factoryFixture, launchInput, MemoryRepository } from './test-fixtures';
const regular: TerminalProfileDto = { id: 'windows-powershell', label: 'Windows PowerShell', environment: 'local', executable: 'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe', args: ['-NoLogo'], distro: null, available: true };
const administrator: TerminalProfileDto = { ...regular, id: ADMIN_POWERSHELL_PROFILE, label: 'Administrator PowerShell', elevation: 'administrator' };
const discovered: TerminalProfilesDto = { profiles: [regular, administrator], defaultProfileId: regular.id, lifetime: 'app-owned', shellSurvival: false };
const value = <T>(result: Result<T>): T => { if (!result.ok) throw new Error(result.error.message); return result.value; };
function fixture() {
  const f = factoryFixture();
  const elevated = Object.assign(new FakePty(), {
    birth: '133000000000000009', whenReady: vi.fn(async () => { elevated.pid = 789; }),
    closeConfirmed: vi.fn(async () => { elevated.exit(0, 0); }),
  });
  const elevate = vi.fn(() => elevated);
  const backend = new PtyBackend({ ...f.options, discover: async () => discovered, elevatedFactory: elevate });
  return { ...f, backend, elevated, elevate };
}
it('discovers without elevating and routes only the explicit administrator tab through the helper', async () => {
  const f = fixture(); value(await f.backend.initialize()); expect(f.elevate).not.toHaveBeenCalled();
  const input = { ...launchInput(), profileId: administrator.id, cwd: "C:\\a';& folder", env: [{ name: 'CUSTOM', value: 'test' }] };
  const owned = value(await f.backend.launch(input));
  expect(f.factory).not.toHaveBeenCalled(); expect(f.elevate).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ cwd: input.cwd, cols: 80, rows: 24, env: expect.objectContaining({ CUSTOM: 'test' }) }));
  expect(owned.root).toMatchObject({ pid: 789, authenticatedBirth: 'windows:133000000000000009', environment: 'local' });
  expect(owned.root.ancestorPid).toBeUndefined(); expect(owned.cleanupPending).toBe(true);
  value(await f.backend.launch({ ...launchInput(), profileId: regular.id, cwd: 'C:\\work' }));
  expect(f.factory).toHaveBeenCalledTimes(1); expect(f.elevate).toHaveBeenCalledTimes(1);
  await f.backend.dispose(); expect(f.elevated.closeConfirmed).toHaveBeenCalledTimes(1);
});
it('retains ownership on unconfirmed helper cleanup and never falls back to a standard PTY', async () => {
  const f = fixture(); value(await f.backend.initialize());
  const input = { ...launchInput(), profileId: administrator.id };
  value(await f.backend.launch(input));
  f.elevated.closeConfirmed.mockRejectedValueOnce(new Error('Disconnected'));
  expect(await f.backend.close({ tabId: input.tabId, generation: input.generation })).toMatchObject({ ok: false, error: { code: 'MONITOR_UNAVAILABLE' } });
  expect(f.backend.get(input.tabId)?.cleanupPending).toBe(true);
  expect(await f.backend.launch({ ...input, generation: randomUUID() })).toMatchObject({ ok: false });
  expect(f.factory).not.toHaveBeenCalled(); await f.backend.dispose();
});
it('can cancel pending UAC startup during shutdown without losing the owned helper', async () => {
  const f = fixture(); value(await f.backend.initialize());
  let reject!: (error: Error) => void;
  f.elevated.whenReady.mockImplementation(() => new Promise<void>((_, no) => { reject = no; }));
  f.elevated.closeConfirmed.mockImplementation(async () => { reject(new Error('UAC cancelled.')); f.elevated.exit(1, 0); });
  const input = { ...launchInput(), profileId: administrator.id };
  const launch = f.backend.launch(input);
  for (let i = 0; i < 10 && !f.elevate.mock.calls.length; i++) await Promise.resolve();
  expect(f.backend.live()).toHaveLength(1);
  await f.backend.dispose(); expect(await launch).toMatchObject({ ok: false, error: { code: 'LAUNCH_FAILED' } });
  expect(f.elevated.closeConfirmed).toHaveBeenCalledOnce(); expect(f.factory).not.toHaveBeenCalled();
});
it('reports UAC denial and keeps the session error without retaining a closed tab', async () => {
  const f = fixture(), repository = new MemoryRepository();
  const service = new EmbeddedSessionService(repository, f.backend, () => ({ setWatch: vi.fn(), dispose: vi.fn() }));
  await service.initialize();
  const session = value(await service.createSession({ cwd: 'C:\\\\work', requestId: randomUUID() }));
  f.elevated.whenReady.mockImplementation(async () => { f.elevated.exit(1, 0); throw new Error('Administrator PowerShell was cancelled.'); });
  expect(await service.addTab({ sessionId: session.id, profileId: administrator.id })).toMatchObject({ ok: false, error: { code: 'LAUNCH_FAILED', message: 'Administrator PowerShell was cancelled.' } });
  expect(repository.tabs(session.id).map(t => t.id)).toEqual([session.tabs[0].id]);
  expect(repository.session(session.id)?.error).toMatchObject({ code: 'LAUNCH_FAILED' });
  await service.dispose();
});

it('refuses administrator execution on non-Windows platforms', async () => {
  const f = fixture(); const backend = new PtyBackend({ ...f.options, platform: 'linux', discover: async () => discovered, elevatedFactory: f.elevate });
  value(await backend.initialize()); expect(await backend.launch({ ...launchInput(), profileId: administrator.id })).toMatchObject({ ok: false, error: { code: 'UNSUPPORTED' } });
  expect(f.elevate).not.toHaveBeenCalled(); expect(f.factory).not.toHaveBeenCalled(); await backend.dispose();
});
it('keeps normal creation/reopening nonadmin and refuses administrator as the saved default', async () => {
  const f = fixture(), repository = new MemoryRepository();
  const service = new EmbeddedSessionService(repository, f.backend, () => ({ setWatch: vi.fn(), dispose: vi.fn() }));
  await service.initialize();
  expect(await service.saveSettings({ ...repository.settings(), terminalProfileId: administrator.id })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
  const session = value(await service.createSession({ cwd: 'C:\\work', requestId: randomUUID() }));
  expect(f.elevate).not.toHaveBeenCalled();
  value(await service.addTab({ sessionId: session.id, profileId: administrator.id }));
  expect(f.elevate).toHaveBeenCalledOnce();
  await service.dispose();
});
