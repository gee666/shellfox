import { chmod, rename, stat, unlink, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { expect } from 'vitest';
import { requestControl } from './control-client';
import { controlEndpointPath } from './control-protocol';

export async function assertControlPermissions(directory: string, endpoint: unknown): Promise<void> {
  const file = controlEndpointPath(directory);
  if (process.platform === 'win32') {
    // Replace with an inherited, unprotected file. Client must fail closed.
    const backup = file + '.backup', inherited = file + '.inherited';
    await writeFile(inherited, JSON.stringify(endpoint)); await rename(file, backup); await rename(inherited, file);
    try { await expect(requestControl(directory, { action: 'list' })).rejects.toThrow('unsafe'); }
    finally { await unlink(file); await rename(backup, file); }
    expect((await requestControl(directory, { action: 'list' })).sessions).toHaveLength(1);
  } else {
    expect((await stat(file)).mode & 0o777).toBe(0o600);
    expect((await stat(path.dirname(file))).mode & 0o777).toBe(0o700);
    await chmod(file, 0o644);
    try { await expect(requestControl(directory, { action: 'list' })).rejects.toThrow('unsafe'); }
    finally { await chmod(file, 0o600); }
  }
}
