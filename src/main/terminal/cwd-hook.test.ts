import { expect, it } from 'vitest';
import { spawnArguments } from './profiles';
import type { TerminalProfileDto } from '../../shared/contracts';
const profile: TerminalProfileDto = { id: 'windows-powershell', label: 'PowerShell', environment: 'local', executable: 'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe', args: ['-NoLogo'], distro: null, available: true };
it('installs directory reporting independently of CLI completion after loading the normal user profile', () => {
  for (const completion of [false, true]) {
    const launch = spawnArguments(profile, 'C:\\work', 'marker', completion), index = launch.args.indexOf('-EncodedCommand');
    expect(index).toBeGreaterThan(0); expect(launch.args).toContain('-NoExit'); expect(launch.args).not.toContain('-NoProfile');
    const script = Buffer.from(launch.args[index + 1]!, 'base64').toString('utf16le');
    expect(script).toContain('ShellfoxOriginalPrompt'); expect(script.includes('Register-ArgumentCompleter')).toBe(completion);
    expect(script).not.toContain('Set-Location');
  }
});
