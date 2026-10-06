import path from 'node:path';
import { cpSync, existsSync, readFileSync, rmSync } from 'node:fs';
import { rebuild } from '@electron/rebuild';
import { env, root, runNode } from './common.mjs';
Object.assign(process.env, env);
if (!['win32', 'linux', 'darwin'].includes(process.platform) || !['x64', 'arm64'].includes(process.arch)) {
  throw new Error('Native builds require Windows, Linux or macOS with x64/arm64 Node on the target OS/architecture.');
}
const manifest = JSON.parse(readFileSync(path.join(root, 'package.json'), 'utf8'));
const ptyDir = path.join(root, 'node_modules/node-pty');
const prebuild = path.join(ptyDir, 'prebuilds', process.platform + '-' + process.arch);
const sourceBuild = process.env.SHELLFOX_BUILD_FROM_SOURCE === '1' || !existsSync(prebuild);
// node-pty 1.1 uses Node-API. Its target-native Windows/macOS prebuilds do not
// depend on Node's module ABI. Validate in the pinned Electron, never host Node.
if (!sourceBuild) {
  rmSync(path.join(ptyDir, 'build/Release'), { recursive: true, force: true });
  cpSync(prebuild, path.join(ptyDir, 'build/Release'), { recursive: true });
  console.log('Using node-pty Node-API prebuild for ' + process.platform + '-' + process.arch);
}
await rebuild({
  buildPath: root, electronVersion: manifest.devDependencies.electron, arch: process.arch,
  onlyModules: sourceBuild ? ['better-sqlite3', 'node-pty'] : ['better-sqlite3'], force: true,
  cachePath: path.join(root, 'tmp/build-cache/electron-rebuild'),
});
// Copy the matching ConPTY DLL/executable and retain Unix spawn-helper.
runNode('node_modules/node-pty/scripts/post-install.js', [], 60000, { npm_config_arch: process.arch });
runNode('node_modules/electron/cli.js', ['scripts/native-load-smoke.cjs'], 60000, { ELECTRON_RUN_AS_NODE: '1' });
