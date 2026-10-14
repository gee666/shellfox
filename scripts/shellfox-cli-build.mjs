import path from 'node:path';
import { copyFile, mkdir, readFile, rm } from 'node:fs/promises';
import { build } from 'esbuild';
import { stageSshRuntime } from './ssh-runtime.mjs';

/** The same standalone CLI bundle and resources are used by builds and package smoke tests. */
export async function buildShellfoxCli(output) {
  const version = JSON.parse(await readFile('package.json', 'utf8')).version;
  await build({ bundle: true, platform: 'node', format: 'cjs', target: 'node24',
    external: ['electron', 'better-sqlite3', 'node-pty', 'cpu-features', '*.node'],
    loader: { '.sh': 'text', '.ps1': 'text' }, sourcemap: false, minifySyntax: true, logLevel: 'info',
    entryPoints: ['src/main/platform/shellfox-cli-entry.ts'], outfile: path.join(output, 'main', 'cli.cjs'),
    define: { __SHELLFOX_VERSION__: JSON.stringify(version) } });
  await rm(path.join(output, 'util'), { recursive: true, force: true });
  if (process.platform === 'win32') {
    await mkdir(path.join(output, 'util'), { recursive: true });
    await copyFile('node_modules/ssh2/util/pagent.exe', path.join(output, 'util', 'pagent.exe'));
  }
  await copyFile('node_modules/ssh2/LICENSE', path.join(output, 'SSH2-LICENSE'));
  await copyFile('node_modules/hash-wasm/LICENSE', path.join(output, 'HASH-WASM-LICENSE'));
  await stageSshRuntime(output);
}
