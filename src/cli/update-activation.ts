import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { open, realpath, rename, unlink, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { z } from 'zod';
import { readHeadlessInstallation, readPackageJson } from './headless-installation';
import { resolveRuntimeContext } from './runtime-context';

const location = z.string().min(1).max(4096).refine(value => path.isAbsolute(value) && !/[\0\r\n]/.test(value));
const originSchema = z.object({ version: z.literal(1), anchor: location }).strict();
const pointerSchema = z.object({ version: z.literal(1), installation: location }).strict();
export const UPDATE_POINTER = 'headless-update.json';
const ORIGIN = 'headless-update-origin.json';
async function optionalJson(file: string): Promise<unknown | undefined> {
  try { return await readPackageJson(file); }
  catch (error) { if (error instanceof Error && 'code' in error && error.code === 'ENOENT') return undefined; throw error; }
}
function samePath(a: string, b: string): boolean { return process.platform === 'win32' ? a.toLowerCase() === b.toLowerCase() : a === b; }
async function validateVersionPath(anchor: string, installation: string): Promise<void> {
  const root = await realpath(installation), stable = await realpath(anchor);
  if (!samePath(root, installation) || !samePath(stable, anchor) ||
    !samePath(path.dirname(path.dirname(path.dirname(root))), path.dirname(stable)) ||
    path.basename(path.dirname(root)) !== 'installation' ||
    !/^shellfox-headless-\d+\.\d+\.\d+-(win32|darwin|linux)-(x64|arm64)-[A-Za-z0-9]+$/.test(path.basename(path.dirname(path.dirname(root))))) {
    throw new Error('Unsafe headless update installation path. Keep the stable installation and its version directories together.');
  }
}
export async function headlessUpdateAnchor(root: string): Promise<string> {
  const origin = await optionalJson(path.join(root, ORIGIN));
  if (origin === undefined) return root;
  const { anchor } = originSchema.parse(origin);
  await validateVersionPath(anchor, root);
  const stable = await readHeadlessInstallation(anchor), current = await readHeadlessInstallation(root);
  if (stable.platform !== current.platform || stable.arch !== current.arch) throw new Error('Headless update origin does not match this runtime.');
  return anchor;
}
export async function acquireUpdateLock(anchor: string): Promise<() => Promise<void>> {
  const file = path.join(anchor, 'headless-update.lock');
  let handle;
  try { handle = await open(file, 'wx', 0o600); }
  catch { throw new Error(`Headless update is locked or this installation is not writable. If no update is running, remove ${file} and retry.`); }
  return async () => { await handle.close(); await unlink(file); };
}
export async function activateHeadlessUpdate(anchor: string, installation: string): Promise<void> {
  await validateVersionPath(anchor, installation);
  await writeFile(path.join(installation, ORIGIN), JSON.stringify({ version: 1, anchor }), { flag: 'wx', mode: 0o600 });
  const temporary = path.join(anchor, `headless-update-${randomUUID()}.json`);
  try {
    const handle = await open(temporary, 'wx', 0o600);
    try { await handle.writeFile(JSON.stringify({ version: 1, installation }) + '\n'); await handle.sync(); }
    finally { await handle.close(); }
    await rename(temporary, path.join(anchor, UPDATE_POINTER));
  } finally { await unlink(temporary).catch(error => { if (error.code !== 'ENOENT') throw error; }); }
}
export async function activeHeadlessInstallation(root: string): Promise<string | null> {
  const anchor = await headlessUpdateAnchor(root), raw = await optionalJson(path.join(anchor, UPDATE_POINTER));
  if (raw === undefined) return null;
  const { installation } = pointerSchema.parse(raw);
  await validateVersionPath(anchor, installation);
  if (!samePath(await headlessUpdateAnchor(installation), anchor)) throw new Error('Headless update pointer does not belong to this installation.');
  return samePath(installation, root) ? null : installation;
}

/** Old service definitions still start this entry. Forward once to the active bundled Node. */
export async function forwardHeadlessUpdate(argv: string[], entry: string): Promise<number | null> {
  const context = await resolveRuntimeContext(entry, {});
  if (!context.headless || !context.runtimePath) return null;
  const target = await activeHeadlessInstallation(context.runtimePath);
  if (!target) return null;
  const runtime = await readHeadlessInstallation(target);
  if (runtime.platform !== process.platform || runtime.arch !== process.arch) throw new Error('Activated headless runtime does not match this machine.');
  const env = { ...process.env };
  for (const key of ['ELECTRON_RUN_AS_NODE', 'NODE_OPTIONS', 'NODE_PATH', 'SHELLFOX_RUNTIME_PATH', 'SHELLFOX_RESOURCES_PATH', 'SHELLFOX_PROJECT_ROOT']) delete env[key];
  return new Promise((resolve, reject) => {
    const child = spawn(runtime.node, [runtime.entry, ...argv], { stdio: ['inherit', 'inherit', 'inherit', 'ipc'], shell: false, env, windowsHide: true });
    const signals: NodeJS.Signals[] = process.platform === 'win32' ? ['SIGINT', 'SIGTERM', 'SIGBREAK'] : ['SIGINT', 'SIGTERM', 'SIGHUP'];
    // Windows console Ctrl+C already reaches both processes. kill(SIGINT) would force-kill Node there.
    const listeners = signals.map(signal => ({ signal, listener: () => { if (process.platform !== 'win32') child.kill(signal); } }));
    for (const { signal, listener } of listeners) process.on(signal, listener);
    const clean = () => { for (const { signal, listener } of listeners) process.off(signal, listener); };
    child.once('error', error => { clean(); reject(error); });
    child.once('close', (code, signal) => { clean(); resolve(code ?? (signal === 'SIGINT' ? 130 : 1)); });
  });
}
/** The forwarding process owns the child lifetime, including abrupt parent exit. */
export function watchUpdateParent(): void {
  if (!process.connected || !process.channel) return;
  process.once('disconnect', () => {
    if (process.listenerCount('SIGTERM')) process.emit('SIGTERM');
    else process.exit(1);
  });
  process.channel.unref();
}
