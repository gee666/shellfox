// Desktop SSH on Windows needs an unmodified console Node, not Electron.
import { copyFile, mkdir, readdir, rm } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { verifiedAsset } from './node-runtime.mjs';
export { downloadRuntimeAsset, fileSha256 } from './node-runtime.mjs';
export async function stageSshRuntime(output, { log = console.log, ...options } = {}) {
  const target = path.resolve(output, 'cli-runtime');
  if (process.platform !== 'win32') {
    log(`[ssh-runtime] ${process.platform}-${process.arch}: bundled Windows Node not required`);
    await rm(target, { recursive: true, force: true });
    return;
  }
  await mkdir(target, { recursive: true });
  for (const [asset, name] of [[`win-${process.arch}/node.exe`, 'node.exe'], ['LICENSE', 'NODE-LICENSE']]) {
    const source = await verifiedAsset(asset, { ...options, log, cache: path.resolve('tmp/ssh-runtime-cache') });
    await copyFile(source, path.join(target, name));
  }
  for (const name of await readdir(target)) if (!['node.exe', 'NODE-LICENSE'].includes(name)) await rm(path.join(target, name), { recursive: true, force: true });
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await stageSshRuntime(process.argv[2] ?? 'tmp/build');
