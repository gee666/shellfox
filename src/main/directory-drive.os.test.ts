import { mkdir, mkdtemp, rmdir } from 'node:fs/promises';
import path from 'node:path';
import { it, expect } from 'vitest';
import { validateDirectory } from './directory';

// HOLD: real Windows drive-type verification needs an approved original owner.
it.runIf(process.platform === 'win32')('verifies a real local project fixture with Windows drive-type policy', async () => {
  await mkdir('tmp', { recursive: true });
  const directory = await mkdtemp(path.resolve('tmp/local-drive-'));
  await expect(validateDirectory(directory)).resolves.toBe(directory);
  await rmdir(directory);
});
