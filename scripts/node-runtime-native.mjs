// Stage in a separate tree. Electron's installed addons are never modified.
import { createRequire } from 'node:module';
import { cpSync, copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, chmodSync } from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { env, root } from './common.mjs';
import { stageNodeHeaders } from './node-runtime.mjs';
const require = createRequire(import.meta.url);
export function nativeCommand(node, args, cwd, extraEnv = {}) {
  const result = spawnSync(node, args, { cwd, env: { ...env, ...extraEnv }, stdio: 'inherit', timeout: 600000, windowsHide: true });
  if (result.error || result.status !== 0) throw new Error(`Headless native command failed in ${cwd}: ${args.join(' ')}; status=${result.status}`, { cause: result.error });
}
export function nativePrebuild(source, name, platform = process.platform, arch = process.arch) {
  const target = `${platform}-${arch}`;
  const file = name === 'node-pty' ? path.join(source, 'prebuilds', target) : path.join(source, 'prebuilds', target + '.node');
  return existsSync(file) ? file : null;
}
export async function stageNodeNativeModules(node, destination, { workName = 'headless-native' } = {}) {
  let headers;
  const work = path.join(root, 'tmp', workName, `${process.platform}-${process.arch}`);
  rmSync(work, { recursive: true, force: true });
  mkdirSync(work, { recursive: true });
  const nativeEnv = { PATH: path.dirname(node) + path.delimiter + process.env.PATH, npm_config_runtime: 'node', npm_config_arch: process.arch };
  for (const name of ['node-pty', 'better-sqlite3']) {
    const source = path.dirname(require.resolve(name + '/package.json'));
    const target = path.join(work, name);
    cpSync(source, target, { recursive: true, dereference: true, filter: file => !['node_modules', 'build', 'prebuilds'].includes(path.relative(source, file).split(path.sep)[0]) });
    // Both pinned packages use Node-API. npm's lockfile-integrity-checked target
    // prebuilds work in Node, independent of Electron's V8 module ABI. Never
    // copy the repository build/Release tree, which may contain Electron builds.
    const prebuild = process.env.SHELLFOX_BUILD_FROM_SOURCE === '1' ? null : nativePrebuild(source, name);
    if (prebuild) {
      mkdirSync(path.join(target, 'build/Release'), { recursive: true });
      if (name === 'node-pty') cpSync(prebuild, path.join(target, 'build/Release'), { recursive: true });
      else copyFileSync(prebuild, path.join(target, 'build/Release/better_sqlite3.node'));
      console.log(`[node-native] ${name}: npm Node-API prebuild ${process.platform}-${process.arch}`);
    } else {
      headers ??= await stageNodeHeaders();
      const gyp = createRequire(require.resolve('@electron/rebuild')).resolve('node-gyp/bin/node-gyp.js');
      const localRequire = createRequire(path.join(source, 'package.json'));
      const addonApi = path.dirname(localRequire.resolve('node-addon-api/package.json'));
      cpSync(addonApi, path.join(target, 'node_modules/node-addon-api'), { recursive: true, dereference: true });
      nativeCommand(node, [gyp, 'rebuild', '--release', `--nodedir=${headers}`, `--arch=${process.arch}`, '--force_build=1'], target, nativeEnv);
    }
    if (name === 'node-pty') {
      nativeCommand(node, [path.join(target, 'scripts/post-install.js')], target, nativeEnv);
      if (process.platform === 'darwin') chmodSync(path.join(target, 'build/Release/spawn-helper'), 0o755);
    }
    const output = path.join(destination, name);
    cpSync(target, output, { recursive: true, filter: file => {
      const relative = path.relative(target, file).replaceAll('\\', '/');
      if (!relative || relative === 'package.json' || /^(LICENSE|COPYING)/i.test(relative)) return true;
      if (relative === 'lib' || relative.startsWith('lib/')) return !/\.test\.|\.map$/.test(relative);
      if (relative === 'build' || relative === 'build/Release') return true;
      return relative.startsWith('build/Release/') && (relative === 'build/Release/conpty' || /\.(node|dll|exe)$/.test(relative) || relative.endsWith('/spawn-helper'));
    } });
    const manifest = JSON.parse(readFileSync(path.join(output, 'package.json'), 'utf8'));
    console.log(`[node-native] staged ${manifest.name}@${manifest.version}`);
  }
  // Validate both selected addons in the actual packaged Node before proceeding.
  const main = path.resolve(destination, '..', 'main.cjs');
  nativeCommand(node, [path.join(root, 'scripts/node-runtime-native-smoke.cjs'), main, path.resolve(destination)], work, nativeEnv);
}
