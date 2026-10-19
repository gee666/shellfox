import { expect, it } from 'vitest';
import type { TerminalProfileDto } from '../../shared/contracts';
import { spawnArguments } from './profiles';
import { powershellCompletion } from '../platform/shellfox-completion';

const profile: TerminalProfileDto = { id: 'pwsh', label: 'PowerShell', executable: 'C:\\Program Files\\PowerShell\\7\\pwsh.exe', args: ['-NoLogo'], environment: 'local', distro: null, available: true };
it('loads PowerShell completion after profiles, stays interactive and leaves the discovered profile unchanged', () => {
  const launch = spawnArguments(profile, 'C:\\work', 'marker', true);
  expect(launch.args.slice(0, 3)).toEqual(['-NoLogo', '-NoExit', '-EncodedCommand']);
  expect(Buffer.from(launch.args[3], 'base64').toString('utf16le')).toBe(powershellCompletion());
  expect(launch.args).not.toContain('-NoProfile');
  expect(profile.args).toEqual(['-NoLogo']);
  expect(spawnArguments(profile, 'C:\\work', 'marker').args).toEqual(['-NoLogo']);
});
it('does not replace Unix or WSL login startup and never sends setup through terminal input', () => {
  const unix = { ...profile, id: 'login-shell', executable: '/bin/bash', args: ['-l', '-i'] };
  expect(spawnArguments(unix, '/work', 'marker', true).args).toEqual(['-l', '-i']);
  expect(spawnArguments({ ...unix, executable: '/bin/zsh' }, '/work', 'marker', true).args).toEqual(['-l', '-i']);
  const wsl = { ...profile, environment: 'wsl' as const, distro: 'Debian', executable: 'wsl.exe' };
  expect(spawnArguments(wsl, '/work', 'marker', true).args).toEqual(['--distribution', 'Debian', '--cd', '/work', '--exec', '/usr/bin/env', 'SHELLFOX_TERMINAL_MARKER=marker', '/bin/bash', '-l', '-i']);
});
