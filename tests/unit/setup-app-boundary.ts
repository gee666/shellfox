import { afterAll, afterEach, vi } from 'vitest';
import { takeBlockedProcesses } from './app-process-model';
import { finishPermissionFixtures } from './endpoint-permission-model';
import { finishDirectoryDriveChecks } from './directory-drive-model';
import { finishFixturePython } from './fixture-python-model';
import { finishFixtureUploadRoots } from './upload-permission-model';

// Vitest's application module graph only. No global process, loader or worker patch.
vi.mock('node:child_process', async () => (await import('./app-process-model')).unitChildProcess);
vi.mock('child_process', async () => (await import('./app-process-model')).unitChildProcess);
vi.mock('node:module', async () => (await import('./app-process-model')).unitModule);
vi.mock('module', async () => (await import('./app-process-model')).unitModule);
vi.mock('node-pty', async () => (await import('./app-process-model')).unitPty);
vi.mock('../../src/main/ssh/agents', async () => (await import('./app-process-model')).unitAgent);
vi.mock('../../src/main/directory', async importOriginal => {
  const { validateDirectory } = await importOriginal<typeof import('../../src/main/directory')>();
  const { withDirectoryDriveCheck } = await import('./directory-drive-model');
  return { validateDirectory: (target: string) => withDirectoryDriveCheck(target, () => validateDirectory(target)) };
});
vi.mock('../../src/main/terminal/python', async importOriginal => {
  const { resolvePython } = await importOriginal<typeof import('../../src/main/terminal/python')>();
  const { fixturePythonResolver } = await import('./fixture-python-model');
  return { resolvePython: fixturePythonResolver(resolvePython) };
});
vi.mock('../../src/main/remote/upload-permissions', async importOriginal => {
  const permissions = await importOriginal<typeof import('../../src/main/remote/upload-permissions')>();
  const { fixtureUploadPermissions } = await import('./upload-permission-model');
  return { ...permissions, protectWindowsUploadDirectory: fixtureUploadPermissions(permissions.protectWindowsUploadDirectory) };
});
vi.mock('../../src/main/terminal/tracking', async importOriginal => {
  const tracking = await importOriginal<typeof import('../../src/main/terminal/tracking')>();
  const getProcessTrackingCapability: typeof tracking.getProcessTrackingCapability = (options = {}, signal) => {
    // Default service fixtures do not opt into a Darwin helper probe. Explicit tracking models keep their own options and doubles.
    if (process.platform === 'darwin' && Object.keys(options).length === 0) {
      return Promise.resolve({ available: false, reason: 'No Darwin tracking helper in the unit fixture.', helperPath: null });
    }
    return tracking.getProcessTrackingCapability(options, signal);
  };
  return { ...tracking, getProcessTrackingCapability };
});

function assertNoUnexpectedAttempt() {
  const unexpected = takeBlockedProcesses();
  if (unexpected.length) throw new Error('Unexpected application process attempt in unit fixture: ' + unexpected.join(', '));
}
afterEach(assertNoUnexpectedAttempt);
afterAll(() => {
  assertNoUnexpectedAttempt();
  finishPermissionFixtures();
  finishDirectoryDriveChecks();
  finishFixturePython();
  finishFixtureUploadRoots();
});
