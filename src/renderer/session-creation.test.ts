import { expect, it } from 'vitest';
import { directorySessionTitle } from './session-creation';

it.each([
  ['win32', 'C:\\', 'C:\\'],
  ['win32', 'P:/', 'P:/'],
  ['win32', String.raw`\\host\share` + '\\', String.raw`\\host\share` + '\\'],
  ['win32', String.raw`\\wsl.localhost\Debian\tmp`, 'tmp'],
  ['linux', '/', '/'],
  ['linux', String.raw`/home/test/back\slash/`, String.raw`back\slash`],
  ['darwin', '/Users/test/project/', 'project'],
])('uses the directory name or the full root on %s: %s', (platform, cwd, title) => {
  expect(directorySessionTitle(cwd, platform)).toBe(title);
});
