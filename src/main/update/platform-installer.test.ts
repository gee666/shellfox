import { mkdir, mkdtemp, rm } from 'node:fs/promises';
import path from 'node:path';
import { expect, it } from 'vitest';
import { assertWindowsHandoffs } from './platform-installer-test-fixture';
import { createUpdatePlatform } from './platform-installer';

it.each(['win32', 'linux', 'darwin'])('disables self-installation in development on %s', async platform => {
  expect(await createUpdatePlatform({ platform, arch: 'x64', executable: '/fake/Shellfox', packaged: false })).toMatchObject({ supported: false });
});
it('does not try Windows x64 installers on ARM64 or portable ZIP copies', async () => {
  for (const input of [{ arch: 'arm64', executable: 'C:\\Shellfox\\app-0.2.5\\Shellfox.exe' }, { arch: 'x64', executable: 'C:\\Downloads\\Shellfox.exe' }]) {
    expect(await createUpdatePlatform({ ...input, platform: 'win32', packaged: true })).toMatchObject({ supported: false });
  }
});
it.each(['ia32', 'riscv64'])('rejects unpublished architecture %s', async arch => {
  expect(await createUpdatePlatform({ platform: 'linux', arch, executable: '/fake', packaged: true })).toMatchObject({ supported: false });
});
it.skipIf(process.platform !== 'win32')('uses fresh handoff controls on every Windows attempt and only installs after commit and PID exit', async () => {
  await assertWindowsHandoffs();
});
it('does not replace arbitrary macOS executables or read-only/unidentified bundles', async () => {
  await mkdir('tmp', { recursive: true });
  const root = await mkdtemp(path.resolve('tmp/updater-unidentified-'));
  try {
    for (const executable of [`${root}/Shellfox`, `${root}/Shellfox.app/Contents/MacOS/Shellfox`]) expect(await createUpdatePlatform({ platform: 'darwin', arch: 'arm64', executable, packaged: true })).toMatchObject({ supported: false });
  } finally { await rm(root, { recursive: true, force: true }); }
});
