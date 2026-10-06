import { describe, expect, it } from 'vitest';
import { parseWslUnc } from './wsl-path';
import { pathSchema } from './schemas';

describe('WSL UNC paths', () => {
  it.each([
    [String.raw`\\wsl.localhost\Debian\var\www`, { distro: 'Debian', guestPath: '/var/www' }],
    [String.raw`\\wsl$\Debian\var\www`, { distro: 'Debian', guestPath: '/var/www' }],
    [String.raw`\\WSL.LOCALHOST\debian\var\www`, { distro: 'debian', guestPath: '/var/www' }],
    [String.raw`\\wsl$\Debian`, { distro: 'Debian', guestPath: '/' }],
    [String.raw`\\wsl.localhost\Debian` + '\\', { distro: 'Debian', guestPath: '/' }],
    [String.raw`\\wsl$\Debian\a folder` + '\\', { distro: 'Debian', guestPath: '/a folder/' }],
  ])('parses %s', (cwd, expected) => {
    expect(parseWslUnc(cwd)).toEqual(expected);
    expect(pathSchema.safeParse(cwd).success).toBe(true);
  });

  it.each([String.raw`C:\work`, '/var/www', String.raw`\\server\share\work`, String.raw`\\wsl.localhost`, String.raw`\\wsl$\\var`, String.raw`\\wsl.localhost.example\Debian\work`])('does not treat %s as a WSL share', cwd => {
    expect(parseWslUnc(cwd)).toBeNull();
  });
});
