import path from 'node:path';
import { constants } from 'node:fs';
import { chmod, lstat, mkdir, open, rename, unlink } from 'node:fs/promises';
import { randomBytes } from 'node:crypto';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { controlEndpointPath, controlEndpointSchema } from './control-protocol';
import { z } from 'zod';
import { hostResourcePaths } from '../host/paths';

const run = promisify(execFile);
const permissionsResultSchema = z.object({ ok: z.literal(true) }).strict();
async function windowsPermissions(file: string, directory: boolean, protect: boolean): Promise<void> {
  try {
    const { stdout, stderr } = await run(hostResourcePaths().tailscale,
      ['--private-permissions', path.resolve(file), directory ? 'directory' : 'file', protect ? 'protect' : 'check'],
      { windowsHide: true, timeout: 8000, maxBuffer: 1024 });
    if (stderr !== '') throw new Error('Unexpected permissions helper output.');
    permissionsResultSchema.parse(JSON.parse(stdout));
  } catch {
    // Do not expose process errors, which include arguments and native output.
    throw new Error('Unsafe control endpoint permissions or unavailable permissions helper.');
  }
}
async function checkPrivate(file: string, directory: boolean): Promise<void> {
  const info = await lstat(file);
  if (info.isSymbolicLink() || (directory ? !info.isDirectory() : !info.isFile() || info.nlink !== 1)) throw new Error('Unsafe control endpoint file.');
  if (process.platform === 'win32') await windowsPermissions(file, directory, false);
  else if (info.uid !== process.getuid?.() || (info.mode & (directory ? 0o077 : 0o177)) !== 0) throw new Error('Unsafe control endpoint permissions.');
}
export async function publishControlEndpoint(userData: string, endpoint: unknown): Promise<() => Promise<void>> {
  return publishPrivateEndpoint(userData, controlEndpointPath(userData), controlEndpointSchema, endpoint);
}
export async function publishPrivateEndpoint<T extends { secret: string }>(userData: string, file: string, schema: z.ZodType<T>, endpoint: unknown): Promise<() => Promise<void>> {
  const data = schema.parse(endpoint);
  const directory = path.dirname(file);
  await preparePrivateDirectory(userData, directory);
  const staged = path.join(directory, randomBytes(16).toString('hex') + '.json');
  const handle = await open(staged, 'wx', 0o600);
  try {
    if (process.platform === 'win32') await windowsPermissions(staged, false, true);
    await handle.writeFile(JSON.stringify(data), 'utf8');
    await handle.sync(); await handle.close();
    await rename(staged, file);
  } catch (error) { await handle.close().catch(() => {}); await unlink(staged).catch(() => {}); throw error; }
  return async () => {
    // An old instance must never unlink a newer instance's credentials.
    try { if ((await readPrivateEndpoint(file, schema)).secret === data.secret) await unlink(file); }
    catch (error) { if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error; }
  };
}
export async function preparePrivateDirectory(userData: string, directory: string): Promise<void> {
  await mkdir(userData, { recursive: true, mode: 0o700 });
  if (process.platform !== 'win32') {
    const parent = await lstat(userData);
    if (!parent.isDirectory() || parent.isSymbolicLink() || parent.uid !== process.getuid?.() || (parent.mode & 0o022)) throw new Error('Unsafe user data directory for control transport.');
  }
  try { await mkdir(directory, { mode: 0o700 }); }
  catch (error) { if ((error as NodeJS.ErrnoException).code !== 'EEXIST') throw error; await checkPrivate(directory, true); }
  if (process.platform === 'win32') await windowsPermissions(directory, true, true);
  else await chmod(directory, 0o700);
  await checkPrivate(directory, true);
}
export async function readControlEndpoint(userData: string) {
  return readPrivateEndpoint(controlEndpointPath(userData), controlEndpointSchema);
}
export async function readPrivateEndpoint<T>(file: string, schema: z.ZodType<T>): Promise<T> {
  await checkPrivate(path.dirname(file), true); await checkPrivate(file, false);
  const handle = await open(file, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
  try {
    const info = await handle.stat();
    if (!info.isFile() || info.size > 4096 || info.nlink !== 1) throw new Error('Invalid control endpoint file.');
    if (process.platform !== 'win32' && (info.uid !== process.getuid?.() || (info.mode & 0o177))) throw new Error('Unsafe control endpoint permissions.');
    const buffer = Buffer.alloc(4097);
    const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0);
    if (bytesRead > 4096) throw new Error('Invalid control endpoint file.');
    return schema.parse(JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(buffer.subarray(0, bytesRead))));
  } finally { await handle.close(); }
}
