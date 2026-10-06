import { randomUUID } from 'node:crypto';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Result, TerminalProfilesDto } from '../../shared/contracts';
import { PtyBackend } from './backend';
import { EmbeddedSessionService } from './service';
import { validateProfileCwd } from './profiles';
import { factoryFixture, MemoryRepository, profiles } from './test-fixtures';

const windowsProfiles: TerminalProfilesDto = {
  ...profiles, defaultProfileId: 'pwsh', profiles: [
    { ...profiles.profiles[0]!, id: 'pwsh', label: 'PowerShell', executable: 'C:\\pwsh.exe' },
    { ...profiles.profiles[0]!, id: 'windows-powershell', label: 'Windows PowerShell', executable: 'C:\\powershell.exe' },
    profiles.profiles[1]!,
    { ...profiles.profiles[1]!, id: 'wsl:Debian', distro: 'Debian', label: 'Debian' },
  ],
};
const unc = String.raw`\\wsl.localhost\Debian\var\www`;
const value = <T,>(result: Result<T>): T => { if (!result.ok) throw new Error(result.error.message); return result.value; };
const services: EmbeddedSessionService[] = [];
const platform = Object.getOwnPropertyDescriptor(process, 'platform')!;
beforeEach(() => Object.defineProperty(process, 'platform', { value: 'win32', configurable: true }));
afterEach(async () => {
  try { for (const service of services.splice(0)) await service.dispose(); }
  finally { Object.defineProperty(process, 'platform', platform); }
});
async function fixture(repository = new MemoryRepository(), discovered = windowsProfiles) {
  const f = factoryFixture();
  const backend = new PtyBackend({ ...f.options, discover: async () => discovered,
    validateCwd: async (profile, cwd) => profile.environment === 'wsl' ? validateProfileCwd(profile, cwd, async () => Buffer.from('')) : cwd });
  const service = new EmbeddedSessionService(repository, backend, () => ({ setWatch() {}, dispose() {} }));
  services.push(service); await service.initialize();
  return { ...f, backend, repository, service, create: async (cwd: string) => value(await service.createSession({ cwd, requestId: randomUUID() })) };
}

describe('cwd-aware implicit terminal profiles', () => {
  it.each([unc, String.raw`\\wsl$\Debian\var\www`, String.raw`\\WSL.LOCALHOST\dEbIaN\var\www`])('starts the matching distro directly for %s and preserves saved UNC paths', async cwd => {
    const f = await fixture(), session = await f.create(cwd);
    expect(session.cwd).toBe(cwd); expect(session.tabs[0]).toMatchObject({ cwd, profileId: 'wsl:Debian' });
    expect(f.repository.session(session.id)?.cwd).toBe(cwd); expect(f.repository.tabs(session.id)[0]!.cwd).toBe(cwd);
    expect(f.backend.get(session.tabs[0]!.id)?.cwd).toBe('/var/www');
    expect(f.factory).toHaveBeenCalledWith(profiles.profiles[1]!.executable,
      ['--distribution', 'Debian', '--cd', '/var/www', '--exec', '/usr/bin/env', expect.stringMatching(/^SHELLFOX_TERMINAL_MARKER=/), '/bin/bash', '-l', '-i'], expect.any(Object));
  });

  it.each(['missing', 'unavailable'] as const)('does not fall back to PowerShell or another distro when Debian is %s', async state => {
    const discovered = { ...windowsProfiles, profiles: windowsProfiles.profiles.flatMap(p => p.id !== 'wsl:Debian' ? [p] : state === 'missing' ? [] : [{ ...p, available: false }]) };
    const f = await fixture(undefined, discovered);
    expect(await f.service.createSession({ cwd: unc, requestId: randomUUID() })).toMatchObject({ ok: false, error: { code: 'DEPENDENCY_MISSING' } });
    expect(f.factory).not.toHaveBeenCalled(); expect(f.repository.sessions()).toHaveLength(0);
    const s = await f.create('C:\\work');
    const saved = f.repository.session(s.id)!; saved.cwd = unc; f.repository.saveSession(saved);
    expect(await f.service.addTab({ sessionId: s.id })).toMatchObject({ ok: false, error: { code: 'DEPENDENCY_MISSING' } });
    f.processes[0]!.exit(0);
    expect(await f.service.activateSession({ sessionId: s.id })).toMatchObject({ ok: false, error: { code: 'DEPENDENCY_MISSING' } });
    expect(f.factory).toHaveBeenCalledTimes(1);
  });

  it('uses PowerShell for Windows directories even with a global WSL default', async () => {
    const f = await fixture();
    f.repository.saveSettings({ ...f.repository.settings(), terminalProfileId: 'wsl:Ubuntu' });
    for (const cwd of [String.raw`C:\work`, 'D:/work']) {
      expect((await f.create(cwd)).tabs[0]!.profileId).toBe('pwsh');
    }
  });

  it('does not reuse an old WSL first tab for a Windows cwd', async () => {
    const f = await fixture(), s = await f.create(unc);
    const saved = f.repository.session(s.id)!; saved.cwd = 'C:\\work'; f.repository.saveSession(saved);
    expect(value(await f.service.addTab({ sessionId: s.id })).tabs.at(-1)!.profileId).toBe('pwsh');
    f.processes.forEach(p => p.exit(0));
    expect(value(await f.service.activateSession({ sessionId: s.id })).tabs.at(-1)!.profileId).toBe('pwsh');
  });

  it('honors a configured local Windows preference and falls back to Windows PowerShell when 7 is absent', async () => {
    const f = await fixture();
    f.repository.saveSettings({ ...f.repository.settings(), terminalProfileId: 'windows-powershell' });
    expect((await f.create('C:\\work')).tabs[0]!.profileId).toBe('windows-powershell');
    expect((await f.create(unc)).tabs[0]!.profileId).toBe('wsl:Debian');
    const fallback = await fixture(undefined, { ...windowsProfiles, profiles: windowsProfiles.profiles.filter(p => p.id !== 'pwsh') });
    expect((await fallback.create('C:\\work')).tabs[0]!.profileId).toBe('windows-powershell');
  });

  it('uses cwd for plus and restart activation despite an existing first PowerShell tab', async () => {
    const f = await fixture(), original = await f.create('C:\\work');
    const saved = f.repository.session(original.id)!; saved.cwd = unc; f.repository.saveSession(saved);
    const added = value(await f.service.addTab({ sessionId: saved.id }));
    expect(added.tabs.map(t => t.profileId)).toEqual(['pwsh', 'wsl:Debian']);
    expect(added.tabs[1]!.cwd).toBe(unc);
    await f.service.dispose();
    const restarted = await fixture(f.repository);
    expect(restarted.factory).not.toHaveBeenCalled();
    const reopened = value(await restarted.service.activateSession({ sessionId: saved.id }));
    expect(reopened.cwd).toBe(unc); expect(reopened.tabs.at(-1)!.profileId).toBe('wsl:Debian');
    value(await restarted.service.activateSession({ sessionId: saved.id }));
    expect(restarted.factory).toHaveBeenCalledTimes(1);
  });

  it('uses an add-tab cwd override and respects explicit profiles without fallback', async () => {
    const f = await fixture(), s = await f.create('C:\\work');
    expect(value(await f.service.addTab({ sessionId: s.id, cwd: unc })).tabs.at(-1)!.profileId).toBe('wsl:Debian');
    expect(value(await f.service.addTab({ sessionId: s.id, cwd: unc, profileId: 'pwsh' })).tabs.at(-1)!.profileId).toBe('pwsh');
    expect(value(await f.service.addTab({ sessionId: s.id, cwd: '/home/user', profileId: 'wsl:Ubuntu' })).tabs.at(-1)!.profileId).toBe('wsl:Ubuntu');
    const calls = vi.mocked(f.factory).mock.calls.length;
    const mismatch = value(await f.service.addTab({ sessionId: s.id, cwd: unc, profileId: 'wsl:Ubuntu' }));
    expect(mismatch.tabs.at(-1)).toMatchObject({ profileId: 'wsl:Ubuntu', error: { code: 'VALIDATION' } });
    expect(await f.service.addTab({ sessionId: s.id, profileId: 'not-discovered' })).toMatchObject({ ok: false, error: { code: 'DEPENDENCY_MISSING' } });
    expect(f.factory).toHaveBeenCalledTimes(calls);
    const unavailable = await fixture(undefined, { ...windowsProfiles, profiles: windowsProfiles.profiles.map(p => p.id === 'wsl:Ubuntu' ? { ...p, available: false } : p) });
    const local = await unavailable.create('C:\\work');
    expect(await unavailable.service.addTab({ sessionId: local.id, profileId: 'wsl:Ubuntu' })).toMatchObject({ ok: false, error: { code: 'DEPENDENCY_MISSING' } });
    expect(unavailable.factory).toHaveBeenCalledTimes(1);
  });

  it('leaves Linux global and existing-tab selection unchanged', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true });
    const repo = new MemoryRepository();
    repo.saveSettings({ ...repo.settings(), adapterId: 'embedded-pty', terminalProfileId: 'login-shell' });
    const f = await fixture(repo, profiles), s = await f.create('/var/www');
    expect(s.tabs[0]!.profileId).toBe('login-shell');
    repo.saveSettings({ ...repo.settings(), terminalProfileId: 'wsl:Ubuntu' });
    expect(value(await f.service.addTab({ sessionId: s.id })).tabs.at(-1)!.profileId).toBe('login-shell');
    f.processes.forEach(p => p.exit(0));
    expect(value(await f.service.activateSession({ sessionId: s.id })).tabs.at(-1)!.profileId).toBe('login-shell');
    // A WSL preference still applies on Linux; no host routing or nested-WSL detection.
    expect((await f.create('/home/user')).tabs[0]!.profileId).toBe('wsl:Ubuntu');
  });
});
