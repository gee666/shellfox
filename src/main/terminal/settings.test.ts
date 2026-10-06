import { describe, expect, it, vi } from 'vitest';
import type { Result, TerminalProfileDto, TerminalProfilesDto } from '../../shared/contracts';
import { PtyBackend } from './backend';
import { EmbeddedSessionService } from './service';
import { factoryFixture, MemoryRepository, profiles } from './test-fixtures';

const windowsPowerShell: TerminalProfileDto = { id: 'windows-powershell', label: 'Windows PowerShell', environment: 'local', executable: 'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe', args: ['-NoLogo'], distro: null, available: true };
const powerShell7: TerminalProfileDto = { ...windowsPowerShell, id: 'pwsh', label: 'PowerShell 7', executable: 'C:\\Program Files\\PowerShell\\7\\pwsh.exe' };
const value = <T>(result: Result<T>): T => { if (!result.ok) throw new Error(result.error.message); return result.value; };
async function fixture(from: string) {
  const f = factoryFixture(), repository = new MemoryRepository(), tracker = { setWatch: vi.fn(), dispose: vi.fn() };
  const discovered: TerminalProfilesDto = { ...structuredClone(profiles), profiles: structuredClone([...profiles.profiles, windowsPowerShell, powerShell7]), defaultProfileId: from };
  const service = new EmbeddedSessionService(repository, new PtyBackend({ ...f.options, discover: async () => discovered }), () => tracker);
  await service.initialize();
  // Establish canonical stored settings, just as a previous successful save would.
  const initial = value(await service.saveSettings({ ...repository.settings(), terminalProfileId: from, shellExecutable: null }));
  return { repository, tracker, service, initial, discovered };
}
describe('canonical embedded profile settings', () => {
  it.each([
    { from: 'login-shell', to: 'wsl:Ubuntu', shellId: 'wsl', executable: 'C:\\Windows\\System32\\wsl.exe' },
    { from: 'windows-powershell', to: 'pwsh', shellId: 'pwsh', executable: powerShell7.executable },
  ] as const)('switches $from -> $to with an unchanged stored executable and returns canonical fields', async ({ from, to, shellId, executable }) => {
    const f = await fixture(from);
    try {
      const draft = { ...f.initial, terminalProfileId: to, accentColor: '#123456', historyPageSize: 47 };
      const saved = value(await f.service.saveSettings(draft));
      expect(saved).toEqual({ ...draft, adapterId: 'embedded-pty', terminalProfileId: to, shellId, shellExecutable: executable });
      expect(f.repository.settings()).toEqual(saved);
      expect(draft.shellExecutable).toBe(f.initial.shellExecutable); // Input DTO is not mutated.
    } finally { await f.service.dispose(); }
  });
  it.each(['login-shell', 'windows-powershell'])('switches twice using the returned %s DTO', async from => {
    const f = await fixture(from), to = from === 'login-shell' ? 'wsl:Ubuntu' : 'pwsh';
    try {
      const first = value(await f.service.saveSettings({ ...f.initial, terminalProfileId: to }));
      const second = value(await f.service.saveSettings({ ...first, terminalProfileId: from }));
      expect(second).toEqual(f.initial); expect(f.repository.settings()).toEqual(second);
      const third = value(await f.service.saveSettings({ ...second, terminalProfileId: to }));
      expect(third).toEqual(first);
    } finally { await f.service.dispose(); }
  });
  it('accepts the selected executable but rejects arbitrary and older-than-stored overrides before watch/persistence', async () => {
    const f = await fixture('login-shell');
    try {
      const before = f.repository.settings(), calls = f.tracker.setWatch.mock.calls.length;
      expect(await f.service.saveSettings({ ...before, terminalProfileId: 'wsl:Ubuntu', shellExecutable: 'C:\\arbitrary.exe' })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
      expect(f.repository.settings()).toEqual(before); expect(f.tracker.setWatch).toHaveBeenCalledTimes(calls);
      const saved = value(await f.service.saveSettings({ ...before, terminalProfileId: 'wsl:Ubuntu', shellExecutable: profiles.profiles[1].executable }));
      expect(saved.shellId).toBe('wsl');
      expect(await f.service.saveSettings({ ...saved, terminalProfileId: 'pwsh', shellExecutable: before.shellExecutable })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
      expect(f.repository.settings()).toEqual(saved);
    } finally { await f.service.dispose(); }
  });
  it('does not authorize an unavailable or unknown profile through the stored executable', async () => {
    const f = await fixture('login-shell');
    try {
      f.discovered.profiles.find(p => p.id === 'pwsh')!.available = false;
      // Backend discovery is retained after initialization; inject availability at the read boundary.
      const read = f.service.backend.getProfiles.bind(f.service.backend);
      vi.spyOn(f.service.backend, 'getProfiles').mockImplementation(() => ({ ...read(), profiles: f.discovered.profiles }));
      for (const terminalProfileId of ['pwsh', 'not-discovered']) expect(await f.service.saveSettings({ ...f.initial, terminalProfileId })).toMatchObject({ ok: false, error: { code: 'VALIDATION' } });
      expect(f.repository.settings()).toEqual(f.initial);
    } finally { await f.service.dispose(); }
  });
});
