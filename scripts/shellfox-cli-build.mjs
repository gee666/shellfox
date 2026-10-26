import path from 'node:path';
import { copyFile, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { build } from 'esbuild';
import { stageSshRuntime } from './ssh-runtime.mjs';
import { stageNodeNativeModules } from './node-runtime-native.mjs';
import { root, runNode } from './common.mjs';
import { writeHeadlessSupport } from './headless-support.mjs';

/** Static bash/zsh completion files, installed system-wide by the Debian package. */
export async function writeCompletionFiles(directory) {
  const bundled = await build({ bundle: true, platform: 'node', format: 'cjs', target: 'node24', write: false, logLevel: 'silent',
    entryPoints: ['src/main/platform/shellfox-completion.ts'] });
  const module = { exports: {} };
  new Function('module', 'exports', bundled.outputFiles[0].text)(module, module.exports);
  const { bashCompletion, zshCompletion } = module.exports;
  await rm(directory, { recursive: true, force: true });
  await mkdir(directory, { recursive: true });
  await writeFile(path.join(directory, 'shellfox.bash'), bashCompletion());
  // zsh autoloads the file as the body of _shellfox; dispatch to the defined completer.
  await writeFile(path.join(directory, '_shellfox'), '#compdef shellfox\n' + zshCompletion() + '_shellfox_complete "$@"\n');
}

/** The same standalone CLI bundle and resources are used by builds and package smoke tests. */
export async function buildShellfoxCli(output) {
  const version = JSON.parse(await readFile('package.json', 'utf8')).version;
  await build({ bundle: true, platform: 'node', format: 'cjs', target: 'node24',
    external: ['electron', 'better-sqlite3', 'node-pty', 'cpu-features', '*.node'],
    loader: { '.sh': 'text', '.ps1': 'text' }, sourcemap: false, minifySyntax: true, logLevel: 'info',
    entryPoints: ['src/main/platform/shellfox-cli-entry.ts'], outfile: path.join(output, 'main', 'cli.cjs'),
    define: { __SHELLFOX_VERSION__: JSON.stringify(version) } });
  await writeCompletionFiles(path.join(output, 'completion'));
  await rm(path.join(output, 'util'), { recursive: true, force: true });
  if (process.platform === 'win32') {
    await mkdir(path.join(output, 'util'), { recursive: true });
    await copyFile('node_modules/ssh2/util/pagent.exe', path.join(output, 'util', 'pagent.exe'));
  }
  await copyFile('node_modules/ssh2/LICENSE', path.join(output, 'SSH2-LICENSE'));
  await copyFile('node_modules/hash-wasm/LICENSE', path.join(output, 'HASH-WASM-LICENSE'));
  await stageSshRuntime(output);
  const modules = path.resolve(output, 'node_modules');
  await rm(modules, { recursive: true, force: true });
  const node = process.platform === 'win32' ? path.resolve(output, 'cli-runtime/node.exe') : process.execPath;
  await stageNodeNativeModules(node, modules, { workName: 'desktop-cli-native' });
  writeHeadlessSupport(path.resolve(output), { version: 1, node: process.platform === 'win32' ? 'cli-runtime/node.exe' : process.platform === 'darwin' ? '../../MacOS/Shellfox' : '../../shellfox', entry: 'main/cli.cjs', resources: '..', runAsNode: process.platform !== 'win32', headless: false });
  // Unix desktop shims use Electron as Node. Check the same isolated Node-API
  // files in that runtime too, rather than relying on the app's ASAR modules.
  if (process.platform !== 'win32') runNode('node_modules/electron/cli.js', [path.join(root, 'scripts/node-runtime-native-smoke.cjs'), path.resolve(output, 'main/cli.cjs'), modules, 'electron'], 30000, { ELECTRON_RUN_AS_NODE: '1' });
}
