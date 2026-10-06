import { describe, it, expect, vi } from 'vitest';
import { discoverProfiles, decodeWsl, validateProfileCwd, spawnArguments } from './profiles';
import { PIDFD_READY } from './unix-close';
import { profiles } from './test-fixtures';
describe('discovered native shell profiles', () => {
  it('keeps an unready WSL distro unavailable with an explicit termination reason', async () => {
    const d = await discoverProfiles({ platform: 'win32', env: {}, exists: async p => p.endsWith('wsl.exe'), run: async (_file, args) => { if (args.includes('--quiet')) return Buffer.from('Ubuntu\n'); throw new Error('no Bash or pidfds'); } });
    expect(d.defaultProfileId).toBeNull(); expect(d.profiles[0]).toMatchObject({ available: false, canTerminateDescendants: false, unavailableReason: expect.stringContaining('pidfd') });
  });
  it('canonicalizes /bin/sh to its actual dash image and requires safe local termination preflight', async () => {
    const d = await discoverProfiles({ platform: 'linux', env: {}, loginShell: '/bin/sh', exists: async () => true, canonical: async p => p === '/bin/sh' ? '/usr/bin/dash' : p, run: async () => Buffer.from(PIDFD_READY) });
    expect(d.profiles[0]).toMatchObject({ executable: '/usr/bin/dash', available: true, canTerminateDescendants: true });
    const missing = await discoverProfiles({ platform: 'linux', env: {}, loginShell: '/bin/sh', exists: async () => true, canonical: async p => p, run: async () => { throw new Error('missing Python'); } });
    expect(missing.profiles.every(p => !p.available && !p.canTerminateDescendants)).toBe(true);
  });
  it('does not permit macOS launch when the bundled supervisor is missing or fails preflight', async () => {
    const d = await discoverProfiles({ platform: 'darwin', env: {}, loginShell: '/bin/zsh', exists: async () => true, canonical: async p => p });
    expect(d.defaultProfileId).toBeNull(); expect(d.profiles[0]).toMatchObject({ available: false, canTerminateDescendants: false, unavailableReason: expect.stringContaining('macOS') });
  });
  it('prefers installed PowerShell and falls back to Windows PowerShell', async () => {
    const both = await discoverProfiles({ platform: 'win32', env: {}, exists: async p => p.endsWith('pwsh.exe') || p.endsWith('powershell.exe') });
    expect(both.defaultProfileId).toBe('pwsh'); expect(both.profiles.map(p => p.id)).toEqual(['pwsh', 'windows-powershell']);
    const fallback = await discoverProfiles({ platform: 'win32', env: {}, exists: async p => p.endsWith('powershell.exe') }); expect(fallback.defaultProfileId).toBe('windows-powershell');
  });
  it('decodes UTF-16 discovery, deduplicates distros and preflights each guest before launch permission', async () => {
    const run = vi.fn(async (_file: string, args: string[]) => args.includes('--quiet') ? Buffer.from('\uFEFFUbuntu\r\nDebian\r\nUbuntu\r\n', 'utf16le') : Buffer.from(PIDFD_READY));
    const discovered = await discoverProfiles({ platform: 'win32', env: {}, exists: async p => p.endsWith('wsl.exe'), run });
    expect(discovered.profiles.map(p => p.id)).toEqual(['wsl:Ubuntu', 'wsl:Debian']); expect(discovered.profiles.every(p => p.available && p.canTerminateDescendants)).toBe(true); expect(run).toHaveBeenCalledTimes(3); expect(run).toHaveBeenCalledWith(expect.stringMatching(/wsl\.exe$/), ['--list', '--quiet']);
    expect(decodeWsl(Buffer.from('Ubuntu\n'))).toBe('Ubuntu\n');
  });
  it('does not report WSL available after failed enumeration', async () => {
    const discovered = await discoverProfiles({ platform: 'win32', env: {}, exists: async p => p.endsWith('wsl.exe'), run: async () => { throw new Error('no WSL'); } }); expect(discovered.profiles).toEqual([]);
  });
  it('uses the actual Unix login shell before SHELL and fallbacks', async () => {
    const d = await discoverProfiles({ platform: 'linux', env: { SHELL: '/bin/bash' }, loginShell: '/bin/zsh', exists: async () => true, canonical: async p => p, run: async () => Buffer.from(PIDFD_READY) }); expect(d.defaultProfileId).toBe('login-shell'); expect(d.profiles[0]).toMatchObject({ executable: '/bin/zsh', args: ['-l', '-i'] });
    const fallback = await discoverProfiles({ platform: 'linux', env: {}, loginShell: '/missing', exists: async p => p === '/bin/bash' || p === '/usr/bin/python3', canonical: async p => p, run: async () => Buffer.from(PIDFD_READY) }); expect(fallback.profiles[0].executable).toBe('/bin/bash'); expect(fallback.defaultProfileId).toBe('shell:/bin/bash');
  });
  it('translates host WSL paths and validates guest directories without interpolation', async () => {
    const run = vi.fn(async (_file: string, args: string[]) => Buffer.from(args.includes('wslpath') ? "/mnt/c/a';& folder\n" : ''));
    const cwd = await validateProfileCwd(profiles.profiles[1], "C:\\a';& folder", run); expect(cwd).toBe("/mnt/c/a';& folder");
    expect(run).toHaveBeenNthCalledWith(1, profiles.profiles[1].executable, ['--distribution', 'Ubuntu', '--exec', 'wslpath', '-a', '-u', "C:\\a';& folder"]);
    expect(run).toHaveBeenNthCalledWith(2, profiles.profiles[1].executable, ['--distribution', 'Ubuntu', '--exec', '/bin/sh', '-c', 'test -d "$1" && test -x "$1"', 'shellfox-cwd', cwd]);
    expect(spawnArguments(profiles.profiles[1], cwd, 'marker').args).toContain('SHELLFOX_TERMINAL_MARKER=marker');
  });
  it.each([
    [String.raw`\\wsl.localhost\Ubuntu\home\user`, '/home/user'],
    [String.raw`\\wsl$\uBuNtU\home\user`, '/home/user'],
    [String.raw`\\wsl.localhost\Ubuntu`, '/'],
  ])('maps %s only into its own discovered guest', async (cwd, guest) => {
    const run = vi.fn(async () => Buffer.from(''));
    expect(await validateProfileCwd(profiles.profiles[1], cwd, run)).toBe(guest);
    expect(run).toHaveBeenCalledWith(profiles.profiles[1].executable, expect.arrayContaining(['Ubuntu', guest]));
    run.mockClear();
    await expect(validateProfileCwd(profiles.profiles[1], String.raw`\\wsl.localhost\Debian\home\user`, run)).rejects.toThrow('different WSL');
    expect(run).not.toHaveBeenCalled();
  });
  it('rejects bad guest cwd and failed accessibility checks', async () => {
    await expect(validateProfileCwd(profiles.profiles[1], '/guest', async () => { throw new Error('not accessible'); })).rejects.toThrow();
    await expect(validateProfileCwd(profiles.profiles[1], 'relative', async () => Buffer.from(''))).rejects.toThrow();
    await expect(validateProfileCwd(profiles.profiles[1], 'C:\\work', async () => Buffer.from('relative'))).rejects.toThrow();
  });
});
