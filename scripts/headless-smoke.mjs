import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { smokeCliHost } from './cli-host-smoke.mjs';
import { env, root } from './common.mjs';
import { headlessDirectory } from './headless-build.mjs';
import { headlessInventory } from './headless-inventory.mjs';
import { nodeVersion } from './node-runtime.mjs';
import { sourceDigest } from './source-digest.mjs';

export async function smokeHeadless(directory = headlessDirectory()) {
  directory = path.resolve(directory);
  const build = JSON.parse(readFileSync(path.join(directory, 'build.json'), 'utf8'));
  assert.equal(build.sourceHash, sourceDigest(root), 'Headless sources are stale; rebuild first.');
  const files = headlessInventory(directory);
  console.log(`[headless-smoke] ${files.length} files, no UI runtime`);
  const node = path.join(directory, 'bin', process.platform === 'win32' ? 'node.exe' : 'node');
  const main = path.join(directory, 'app/main.cjs');
  const childEnv = { ...env, SHELLFOX_HEADLESS: '1', SHELLFOX_RESOURCES_PATH: path.join(directory, 'resources'), SHELLFOX_RUNTIME_PATH: directory };
  for (const key of ['DISPLAY', 'WAYLAND_DISPLAY', 'ELECTRON_RUN_AS_NODE', 'NODE_PATH', 'NODE_OPTIONS']) delete childEnv[key];
  function command(args, timeout = 15000) {
    const result = spawnSync(node, args, { cwd: directory, env: childEnv, timeout, encoding: 'utf8', windowsHide: true });
    assert.equal(result.error, undefined, result.error?.message);
    assert.equal(result.status, 0, result.stderr || result.stdout);
    return result.stdout;
  }
  assert.equal(command(['--version']).trim(), 'v' + nodeVersion);
  assert.deepEqual(JSON.parse(command(['-p', 'JSON.stringify({platform:process.platform,arch:process.arch,electron:process.versions.electron??null})'])), { platform: process.platform, arch: process.arch, electron: null });
  command([main, '--help']);
  // Real SQLite and PTY operations, loaded from this tree with the shipped Node.
  command([path.join(root, 'scripts/node-runtime-native-smoke.cjs'), main, path.join(directory, 'app/node_modules')], 20000);
  const launcher = process.platform === 'win32' ? 'powershell.exe' : path.join(directory, 'shellfox');
  const launcherArgs = process.platform === 'win32' ? ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', path.join(directory, 'shellfox.ps1'), '--help'] : ['--help'];
  const launcherTimeout = process.platform === 'win32' ? 60000 : 20000;
  const launched = spawnSync(launcher, launcherArgs, { cwd: root, env: childEnv, timeout: launcherTimeout, encoding: 'utf8', windowsHide: true });
  assert.ifError(launched.error);
  assert.equal(launched.status, 0, `signal=${launched.signal ?? 'none'} ${launched.stderr}`);
  await smokeCliHost({ executable: node, entry: main, cwd: root, environment: childEnv });
  assert.equal(build.sourceHash, sourceDigest(root), 'Sources changed during headless smoke.');
  assert.equal(build.product, 'headless');
  assert.equal(build.platform, process.platform);
  assert.equal(build.arch, process.arch);
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await smokeHeadless(process.argv[2]);
