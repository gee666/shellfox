import assert from 'node:assert/strict';
import { accessSync, constants, existsSync, mkdirSync, readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { listPackage } from '@electron/asar';
import { run, runNode, root } from './common.mjs';
import { darwinHelpers } from './terminal-native-helpers.mjs';
import { sourceDigest } from './source-digest.mjs';
const platform = process.platform, arch = process.arch;
const output = path.join(root, 'tmp/packages', `Shellfox-${platform}-${arch}`);
const app = path.join(output, 'Shellfox.app/Contents');
const resources = platform === 'darwin' ? path.join(app, 'Resources') : path.join(output, 'resources');
const executable = platform === 'darwin' ? path.join(app, 'MacOS/Shellfox') : path.join(output, platform === 'win32' ? 'Shellfox.exe' : 'shellfox');
const asar = path.join(resources, 'app.asar');
assert.ok(existsSync(asar), 'Run pnpm package first on this OS/architecture.');
const stamp = JSON.parse(readFileSync(path.join(root, 'tmp/package-source-stamp.json'), 'utf8'));
assert.equal(stamp.platform, platform);
assert.equal(stamp.arch, arch);
assert.equal(sourceDigest(root), stamp.sourceHash, 'Packaged sources are stale; rebuild after backend changes settle.');
const files = listPackage(asar).map(file => file.replaceAll('\\', '/'));
assert.ok(files.includes('/tmp/build/main/index.cjs'));
assert.ok(files.includes('/tmp/build/preload/index.cjs'));
assert.ok(files.includes('/tmp/build/renderer/index.html'));
assert.ok(!files.some(file => /^\/(src|tests|docs|native|scripts|resources|\.github)(\/|$)/.test(file)), 'Source/test/legacy resources leaked into runtime.');
const unpacked = path.join(resources, 'app.asar.unpacked/node_modules');
const pty = path.join(unpacked, 'node-pty/build/Release');
for (const file of platform === 'win32'
  ? ['conpty.node', 'conpty_console_list.node', 'pty.node', 'winpty.dll', 'winpty-agent.exe', 'conpty/conpty.dll', 'conpty/OpenConsole.exe']
  : platform === 'darwin' ? ['pty.node', 'spawn-helper'] : ['pty.node']) {
  assert.ok(existsSync(path.join(pty, file)), 'Not unpacked: ' + file);
}
assert.ok(existsSync(path.join(unpacked, 'node-pty/lib/conpty_console_list_agent.js')), 'Forked JS agent must be unpacked too.');
if (platform === 'darwin') {
  // Match the updater and @electron/osx-sign's default strict bundle verification,
  // including ad-hoc Forge builds. Do not weaken update verification for previews.
  run('/usr/bin/codesign', ['--verify', '--deep', '--strict', path.dirname(app)], 60000);
  accessSync(path.join(pty, 'spawn-helper'), constants.X_OK);
  for (const { basename } of darwinHelpers) accessSync(path.join(resources, 'terminal-native', basename), constants.X_OK);
  assert.ok(!files.some(file => file.startsWith('/terminal-native/')), 'Native helpers must be outside ASAR.');
  runNode('scripts/mac-packaged-preflight.mjs', [resources, executable], 120000);
}
assert.ok(existsSync(path.join(unpacked, 'better-sqlite3/prebuilds', `${platform}-${arch}.node`)) || existsSync(path.join(unpacked, 'better-sqlite3/build/Release/better_sqlite3.node')));
assert.ok(!readdirSync(resources).some(file => /^(native|shell|win-x64|linux-x64)$/.test(file)), 'Legacy .NET/shell resources must not be shipped.');
const build = readFileSync(path.join(root, 'tmp/build/main/index.cjs'), 'utf8');
assert.ok(!build.includes('manager:test-inspect'), 'Production inspection hook found.');
const data = path.join(root, 'tmp/packaged-native-smoke', randomUUID());
mkdirSync(data, { recursive: true });
run(executable, [path.join(root, 'scripts/packaged-native-smoke.cjs'), asar, data], 60000, { ELECTRON_RUN_AS_NODE: '1' });
runNode('scripts/packaged-backend-smoke.mjs', [asar, executable], 120000);
assert.equal(sourceDigest(root), stamp.sourceHash, 'Sources changed during packaged smoke; rebuild after review.');
console.log('Packaged inventory, unpacked helpers, SQLite, native PTY and real backend smoke passed for ' + platform + '-' + arch + '.');
