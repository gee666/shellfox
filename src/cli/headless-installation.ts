import { constants } from 'node:fs';
import { access, lstat, readFile } from 'node:fs/promises';
import path from 'node:path';
import { z } from 'zod';
const buildSchema = z.object({ product: z.literal('headless'), version: z.string().regex(/^\d+\.\d+\.\d+$/), platform: z.enum(['win32', 'darwin', 'linux']), arch: z.enum(['x64', 'arm64']) });
const runtimeSchema = z.object({ version: z.literal(1), headless: z.literal(true), runAsNode: z.literal(false),
  node: z.enum(['bin/node', 'bin/node.exe']), entry: z.literal('app/main.cjs'), resources: z.literal('resources') }).strict();
export async function readPackageJson(file: string): Promise<unknown> {
  const info = await lstat(file);
  if (!info.isFile() || info.isSymbolicLink() || info.size > 16 * 1024) throw new Error('Unsafe headless package manifest.');
  return JSON.parse(await readFile(file, 'utf8'));
}
const darwinExecutables = ['app/node_modules/node-pty/build/Release/spawn-helper', 'resources/terminal-native/shellfox-process-snapshot', 'resources/terminal-native/shellfox-terminal-supervisor'];
export function requiredHeadlessExecutables(platform: string): string[] {
  return platform === 'win32' ? [] : ['bin/node', 'shellfox', 'resources/tailscale/shellfox-tailscale', ...(platform === 'darwin' ? darwinExecutables : [])];
}
export function requiredHeadlessFiles(platform: string): string[] {
  const files = ['app/main.cjs', 'shellfox', 'shellfox.cmd', 'shellfox.ps1', 'service/install.mjs',
    `bin/node${platform === 'win32' ? '.exe' : ''}`, 'app/node_modules/better-sqlite3/build/Release/better_sqlite3.node',
    'app/node_modules/node-pty/build/Release/pty.node', `resources/tailscale/shellfox-tailscale${platform === 'win32' ? '.exe' : ''}`];
  if (platform === 'linux') files.push('service/shellfox-user.service.in');
  if (platform === 'darwin') files.push(...darwinExecutables, 'service/com.shellfox.host.plist.in');
  if (platform === 'win32') files.push('service/run-host.ps1', 'util/pagent.exe', 'app/node_modules/node-pty/build/Release/conpty.node', 'app/node_modules/node-pty/build/Release/conpty/conpty.dll', 'app/node_modules/node-pty/build/Release/conpty/OpenConsole.exe');
  return files;
}
export async function readHeadlessInstallation(root: string) {
  const build = buildSchema.parse(await readPackageJson(path.join(root, 'build.json')));
  const runtime = runtimeSchema.parse(await readPackageJson(path.join(root, 'runtime.json')));
  if (runtime.node !== (build.platform === 'win32' ? 'bin/node.exe' : 'bin/node')) throw new Error('Headless runtime does not match this platform.');
  for (const name of requiredHeadlessFiles(build.platform)) {
    const file = await lstat(path.join(root, name));
    if (!file.isFile() || file.isSymbolicLink() || file.size === 0) throw new Error('Headless package is incomplete.');
  }
  if (process.platform !== 'win32') {
    for (const name of requiredHeadlessExecutables(build.platform)) {
      try { await access(path.join(root, name), constants.X_OK); }
      catch { throw new Error(`Headless package executable is not runnable: ${name}`); }
    }
  }
  await access(path.join(root, 'resources'));
  return { ...build, node: path.join(root, runtime.node), entry: path.join(root, runtime.entry) };
}
export async function validateHeadlessInstallation(root: string, version: string, platform: string, arch: string): Promise<void> {
  const build = await readHeadlessInstallation(root);
  if (build.version !== version || build.platform !== platform || build.arch !== arch) throw new Error('Headless package identity does not match this release or runtime.');
}
