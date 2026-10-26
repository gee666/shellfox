import { build } from 'esbuild';
import { chmodSync, copyFileSync, cpSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { root, run, runNode } from './common.mjs';
import { stageNodeRuntime, nodeVersion } from './node-runtime.mjs';
import { stageNodeNativeModules } from './node-runtime-native.mjs';
import { sourceDigest } from './source-digest.mjs';
import { buildHelper, helperTarget } from './tailscale.mjs';
import { darwinHelpers } from './terminal-native-helpers.mjs';
import { writeHeadlessSupport } from './headless-support.mjs';
import { headlessInventory } from './headless-inventory.mjs';

export function headlessDirectory() {
  const parent = process.env.SHELLFOX_HEADLESS_OUT_DIR ? path.resolve(process.env.SHELLFOX_HEADLESS_OUT_DIR) : path.join(root, 'tmp/headless');
  const relative = path.relative(path.join(root, 'tmp'), parent);
  if (process.env.SHELLFOX_HEADLESS_OUT_DIR && (!relative || relative.startsWith('..') || path.isAbsolute(relative))) throw new Error('Alternate headless output must be inside project tmp.');
  return path.join(parent, `shellfox-headless-${process.platform}-${process.arch}`);
}
export function launcherFiles() {
  return {
    shellfox: '#!/bin/sh\nset -eu\nroot=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)\nexport SHELLFOX_HEADLESS=1\nexport SHELLFOX_RESOURCES_PATH="$root/resources"\nexport SHELLFOX_RUNTIME_PATH="$root"\nunset ELECTRON_RUN_AS_NODE NODE_PATH NODE_OPTIONS\nexec "$root/bin/node" "$root/app/main.cjs" "$@"\n',
    'shellfox.cmd': '@echo off\r\nsetlocal DisableDelayedExpansion\r\nset "SHELLFOX_HEADLESS=1"\r\nset "SHELLFOX_RESOURCES_PATH=%~dp0resources"\r\nset "SHELLFOX_RUNTIME_PATH=%~dp0"\r\nset "ELECTRON_RUN_AS_NODE="\r\nset "NODE_PATH="\r\nset "NODE_OPTIONS="\r\n"%~dp0bin\\node.exe" "%~dp0app\\main.cjs" %*\r\nexit /b %errorlevel%\r\n',
    'shellfox.ps1': '$ErrorActionPreference = "Stop"\n$env:SHELLFOX_HEADLESS = "1"\n$env:SHELLFOX_RESOURCES_PATH = Join-Path $PSScriptRoot "resources"\n$env:SHELLFOX_RUNTIME_PATH = $PSScriptRoot\nRemove-Item Env:ELECTRON_RUN_AS_NODE, Env:NODE_PATH, Env:NODE_OPTIONS -ErrorAction SilentlyContinue\n$json = ConvertTo-Json -InputObject @($args) -Compress\n$encoded = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($json))\n& (Join-Path $PSScriptRoot "bin/node.exe") (Join-Path $PSScriptRoot "app/main.cjs") --shellfox-argv-json $encoded\nexit $LASTEXITCODE\n',
  };
}
export async function buildHeadless() {
  if (!['win32', 'darwin', 'linux'].includes(process.platform) || !['x64', 'arm64'].includes(process.arch)) throw new Error('Build headless on the native target OS and architecture.');
  const sourceHash = sourceDigest(root);
  const output = headlessDirectory();
  rmSync(output, { recursive: true, force: true });
  mkdirSync(path.join(output, 'app/node_modules'), { recursive: true });
  const node = await stageNodeRuntime(path.join(output, 'bin'));
  const manifest = JSON.parse(readFileSync('package.json', 'utf8'));
  const result = await build({ bundle: true, platform: 'node', format: 'cjs', target: 'node24', metafile: true,
    external: ['better-sqlite3', 'node-pty', 'cpu-features', '*.node'],
    loader: { '.sh': 'text', '.ps1': 'text' }, sourcemap: false, minifySyntax: true,
    entryPoints: ['src/main/platform/shellfox-cli-entry.ts'], outfile: path.join(output, 'app/main.cjs'),
    define: { __SHELLFOX_VERSION__: JSON.stringify(manifest.version) },
    plugins: [{ name: 'no-headless-ui', setup(builder) {
      builder.onResolve({ filter: /^(electron|react|react-dom|@xterm\/xterm|@xterm\/addon-fit)(\/|$)/ }, args => ({ errors: [{ text: 'Headless code must not import ' + args.path }] }));
    } }],
  });
  const forbiddenInput = Object.keys(result.metafile.inputs).find(file => /(^|\/)src\/(renderer|preload)\//.test(file));
  if (forbiddenInput) throw new Error('UI code in headless bundle: ' + forbiddenInput);
  writeFileSync(path.join(output, 'app/package.json'), JSON.stringify({ name: 'shellfox-headless', version: manifest.version, private: true, type: 'commonjs', dependencies: Object.fromEntries(['node-pty', 'better-sqlite3'].map(name => [name, manifest.dependencies[name]])) }, null, 2) + '\n');
  writeFileSync(path.join(root, 'tmp/headless-bundle-inputs.json'), JSON.stringify(result.metafile, null, 2));
  await stageNodeNativeModules(node, path.join(output, 'app/node_modules'));
  if (process.platform === 'darwin') runNode('scripts/terminal-native.mjs', [], 600000);
  const resources = path.join(output, 'resources');
  mkdirSync(resources, { recursive: true });
  if (process.platform === 'darwin') {
    mkdirSync(path.join(resources, 'terminal-native'), { recursive: true });
    for (const { basename } of darwinHelpers) copyFileSync(path.join(root, 'tmp/terminal-native', `darwin-${process.arch}`, basename), path.join(resources, 'terminal-native', basename));
  }
  buildHelper();
  cpSync(helperTarget().directory, path.join(resources, 'tailscale'), { recursive: true });
  // ssh2 resolves ../util from the bundle's directory, as in the desktop CLI.
  if (process.platform === 'win32') {
    mkdirSync(path.join(output, 'util'), { recursive: true });
    copyFileSync('node_modules/ssh2/util/pagent.exe', path.join(output, 'util/pagent.exe'));
  }
  for (const [dependency, notice] of [['ssh2', 'SSH2-LICENSE'], ['hash-wasm', 'HASH-WASM-LICENSE']]) copyFileSync(`node_modules/${dependency}/LICENSE`, path.join(output, notice));
  copyFileSync('LICENSE', path.join(output, 'LICENSE'));
  for (const [name, text] of Object.entries(launcherFiles())) {
    writeFileSync(path.join(output, name), text);
    if (name === 'shellfox') chmodSync(path.join(output, name), 0o755);
  }
  writeHeadlessSupport(output);
  if (process.platform === 'darwin') signHeadless(output);
  writeFileSync(path.join(output, 'build.json'), JSON.stringify({ product: 'headless', sourceHash, version: manifest.version, nodeVersion, platform: process.platform, arch: process.arch, signing: process.platform === 'darwin' ? 'ad-hoc, not notarized' : 'unsigned' }, null, 2) + '\n');
  headlessInventory(output);
  if (sourceDigest(root) !== sourceHash) throw new Error('Sources changed during headless build; rebuild after changes settle.');
  console.log(`[headless] ${output}`);
  return output;
}
function signHeadless(directory) {
  // Sign each Mach-O independently. This archive is not a notarized .app.
  for (const name of readdirSync(directory)) {
    const file = path.join(directory, name);
    if (statSync(file).isDirectory()) signHeadless(file);
    else if (name === 'node' || name.endsWith('.node') || name === 'spawn-helper' || ['shellfox-process-snapshot', 'shellfox-terminal-supervisor', 'shellfox-tailscale'].includes(name)) {
      chmodSync(file, 0o755);
      run('codesign', ['--force', '--sign', '-', '--timestamp=none', file], 60000);
      run('codesign', ['--verify', '--strict', file], 60000);
    }
  }
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await buildHeadless();
