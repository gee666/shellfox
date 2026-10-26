import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmod, copyFile, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { nodeHashes, nodeVersion, runtimeArchive, stageNodeRuntime, verifiedAsset } from './node-runtime.mjs';
import { headlessDirectory, launcherFiles } from './headless-build.mjs';
import { headlessInventory } from './headless-inventory.mjs';
import { writeHeadlessSupport } from './headless-support.mjs';
import { serviceName, systemdEscape, xmlEscape, renderSystemd, readRuntime } from './headless-service.mjs';
await mkdir('tmp', { recursive: true });
async function scratch(body) {
  const directory = await mkdtemp(path.resolve('tmp/headless-contract-'));
  try { await body(directory); } finally { await rm(directory, { recursive: true, force: true }); }
}

test('alternate preview outputs stay under project tmp without replacing a running package', () => {
  const previous = process.env.SHELLFOX_HEADLESS_OUT_DIR;
  try {
    process.env.SHELLFOX_HEADLESS_OUT_DIR = path.resolve('tmp/headless-preview');
    assert.equal(headlessDirectory(), path.resolve('tmp/headless-preview', `shellfox-headless-${process.platform}-${process.arch}`));
    for (const folder of [path.resolve('tmp'), path.resolve('..'), path.resolve('tmp', '..', 'outside')]) {
      process.env.SHELLFOX_HEADLESS_OUT_DIR = folder;
      assert.throws(headlessDirectory, /inside project tmp/);
    }
  } finally { if (previous === undefined) delete process.env.SHELLFOX_HEADLESS_OUT_DIR; else process.env.SHELLFOX_HEADLESS_OUT_DIR = previous; }
});

test('all six target runtimes and build headers have committed checksum pins', () => {
  for (const platform of ['win32', 'darwin', 'linux']) for (const arch of ['x64', 'arm64']) {
    const archive = runtimeArchive(platform, arch);
    assert.match(nodeHashes[archive], /^[a-f0-9]{64}$/);
    assert.ok(archive.includes(nodeVersion));
  }
  assert.throws(() => runtimeArchive('freebsd', 'x64'));
  assert.throws(() => runtimeArchive('linux', 'ia32'));
  for (const file of [`node-v${nodeVersion}-headers.tar.gz`, 'win-x64/node.lib', 'win-arm64/node.lib']) assert.match(nodeHashes[file], /^[a-f0-9]{64}$/);
});
test('runtime archives reject tampering before extraction or execution and clean partial downloads', async () => scratch(async directory => {
  const cache = path.join(directory, 'cache');
  let downloads = 0;
  const options = { cache, download: async (url, file) => { assert.ok(url.startsWith('https://nodejs.org/dist/v' + nodeVersion + '/')); downloads++; await writeFile(file, 'tampered'); }, log: () => {} };
  for (const platform of ['win32', 'darwin', 'linux']) {
    await assert.rejects(stageNodeRuntime(path.join(directory, platform), { ...options, platform, arch: 'x64' }), /checksum mismatch/);
    await assert.rejects(readFile(path.join(directory, platform, platform === 'win32' ? 'node.exe' : 'node')), { code: 'ENOENT' });
  }
  assert.equal(downloads, 3);
  assert.deepEqual(await readdir(cache), []);
  await assert.rejects(verifiedAsset('SHASUMS256.txt', options), /Unsupported/);
}));
test('headless launchers execute packaged Node and preserve arguments and runtime paths', async () => scratch(async directory => {
  const install = path.join(directory, "install space ' & %");
  await mkdir(path.join(install, 'bin'), { recursive: true });
  await mkdir(path.join(install, 'app'));
  await copyFile(process.execPath, path.join(install, 'bin', process.platform === 'win32' ? 'node.exe' : 'node'));
  await writeFile(path.join(install, 'app/main.cjs'), `const args=process.argv[2]==='--shellfox-argv-json'?JSON.parse(Buffer.from(process.argv[3],'base64').toString('utf8')):process.argv.slice(2); console.log(JSON.stringify({ args, headless:process.env.SHELLFOX_HEADLESS, resources:process.env.SHELLFOX_RESOURCES_PATH, runtime:process.env.SHELLFOX_RUNTIME_PATH, electron:process.env.ELECTRON_RUN_AS_NODE, nodePath:process.env.NODE_PATH }));`);
  for (const [name, text] of Object.entries(launcherFiles())) await writeFile(path.join(install, name), text);
  await chmod(path.join(install, 'shellfox'), 0o755);
  const args = ['--user-data', 'two words', 'host', 'run', '--json', 'quoted"value'];
  const command = process.platform === 'win32' ? 'powershell.exe' : path.join(install, 'shellfox');
  const commandArgs = process.platform === 'win32' ? ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', path.join(install, 'shellfox.ps1'), ...args] : args;
  // Cold PowerShell and copied executable startup can exceed 15s on Windows ARM runners.
  const launcherTimeout = process.platform === 'win32' ? 60000 : 15000;
  const result = spawnSync(command, commandArgs, { cwd: directory, timeout: launcherTimeout, encoding: 'utf8', env: { ...process.env, ELECTRON_RUN_AS_NODE: '1', NODE_PATH: 'not-used' } });
  assert.ifError(result.error);
  assert.equal(result.status, 0, `signal=${result.signal ?? 'none'} ${result.stderr}`);
  const value = JSON.parse(result.stdout);
  assert.deepEqual(value.args, args);
  assert.equal(value.headless, '1');
  assert.equal(value.resources, path.join(install, 'resources'));
  assert.equal(path.resolve(value.runtime), install);
  assert.equal(value.electron, undefined);
  assert.equal(value.nodePath, undefined);
  if (process.platform === 'win32') {
    const cmd = spawnSync(process.env.ComSpec || 'cmd.exe', ['/d', '/s', '/c', `""${path.join(install, 'shellfox.cmd')}" --user-data "two words" host run --json"`], { cwd: directory, windowsVerbatimArguments: true, timeout: launcherTimeout, encoding: 'utf8' });
    assert.ifError(cmd.error);
    assert.equal(cmd.status, 0, `signal=${cmd.signal ?? 'none'} ${cmd.stderr}`);
    assert.deepEqual(JSON.parse(cmd.stdout).args, args.slice(0, -1));
  }
}));
async function inventoryFixture(directory, platform) {
  const files = ['app/main.cjs', 'shellfox', 'shellfox.cmd', 'shellfox.ps1', 'README.txt', 'build.json', 'runtime.json', 'service/install.mjs', 'bin/NODE-LICENSE',
    `bin/node${platform === 'win32' ? '.exe' : ''}`, 'app/node_modules/better-sqlite3/build/Release/better_sqlite3.node', 'app/node_modules/node-pty/build/Release/pty.node',
    `resources/tailscale/shellfox-tailscale${platform === 'win32' ? '.exe' : ''}`, 'resources/tailscale/THIRD-PARTY-NOTICES.txt'];
  if (platform === 'darwin') files.push('app/node_modules/node-pty/build/Release/spawn-helper', 'resources/terminal-native/shellfox-process-snapshot', 'resources/terminal-native/shellfox-terminal-supervisor');
  if (platform === 'win32') files.push('util/pagent.exe', 'app/node_modules/node-pty/build/Release/conpty.node', 'app/node_modules/node-pty/build/Release/conpty/conpty.dll', 'app/node_modules/node-pty/build/Release/conpty/OpenConsole.exe');
  for (const file of files) { await mkdir(path.dirname(path.join(directory, file)), { recursive: true }); await writeFile(path.join(directory, file), 'fixture'); }
  await writeFile(path.join(directory, 'app/package.json'), JSON.stringify({ dependencies: { 'node-pty': '1', 'better-sqlite3': '1' } }));
}
test('inventory requires native runtimes and rejects renderer, Electron and UI dependencies on every OS', async () => scratch(async directory => {
  for (const platform of ['win32', 'darwin', 'linux']) {
    const target = path.join(directory, platform);
    await inventoryFixture(target, platform);
    assert.ok(headlessInventory(target, platform).includes('app/main.cjs'));
    await mkdir(path.join(target, 'resources/elevated-terminal'), { recursive: true });
    await writeFile(path.join(target, 'resources/elevated-terminal/Shellfox.ElevatedTerminal.dll'), 'unused runtime');
    assert.throws(() => headlessInventory(target, platform), /Unused administrator runtime/);
    await rm(path.join(target, 'resources/elevated-terminal'), { recursive: true });
    await writeFile(path.join(target, 'app/index.html'), '<html>');
    assert.throws(() => headlessInventory(target, platform), /UI file/);
    await rm(path.join(target, 'app/index.html'));
    await writeFile(path.join(target, 'app/package.json'), '{"dependencies":{"react":"19"}}');
    assert.throws(() => headlessInventory(target, platform), /UI dependency/);
    await writeFile(path.join(target, 'app/package.json'), '{}');
    await rm(path.join(target, 'bin', platform === 'win32' ? 'node.exe' : 'node'));
    assert.throws(() => headlessInventory(target, platform), /Missing headless package file/);
  }
}));
test('same-user startup resources are executable installers, include graceful stop and never register Node with SCM', async () => scratch(async directory => {
  writeHeadlessSupport(directory);
  const installer = await readFile(path.join(directory, 'service/install.mjs'), 'utf8');
  assert.ok(installer.includes("['--user', 'enable', '--now', name]"));
  assert.ok(installer.includes("['bootstrap', domain, file]"));
  assert.ok(installer.includes('Register-ScheduledTask'));
  assert.ok(installer.includes('-LogonType Interactive -RunLevel Limited'));
  assert.ok(installer.includes('-LogonType S4U -RunLevel Limited'));
  assert.ok(installer.includes('New-ScheduledTaskTrigger -AtStartup'));
  assert.ok(installer.includes("'host', 'stop', '--yes', '--json'"));
  assert.ok(!installer.includes('LocalSystem') && !installer.includes('sc.exe') && !installer.includes('taskkill') && !installer.includes('Stop-Process'));
  const system = await readFile(path.join(directory, 'service/shellfox-system.service.in'), 'utf8');
  assert.ok(system.includes('User=@USER@') && !system.includes('User=root'));
  assert.ok(system.includes('KillMode=control-group'));
  const docs = await readFile(path.join(directory, 'README.txt'), 'utf8');
  assert.ok(docs.includes('notarization') && docs.includes('NOT an SCM') && docs.includes('S4U'));
  assert.equal(systemdEscape('/path % "quoted"'), '/path %% \\"quoted\\"');
  assert.equal(xmlEscape('/a&<>"'), '/a&amp;&lt;&gt;&quot;');
  assert.equal(renderSystemd('ExecStart="@INSTALL_ROOT@/bin/node" --user-data "@DATA_ROOT@"\nEnvironment="ROOT=@INSTALL_ROOT@"', '/root $value %', '/data $state'), 'ExecStart="/root $$value %%/bin/node" --user-data "/data $$state"\nEnvironment="ROOT=/root $value %%"');
  assert.equal(serviceName(directory), serviceName(directory));
  assert.notEqual(serviceName(directory), serviceName(directory + '-other'));
}));
test('desktop startup manifests bind services to the desktop CLI without a fake headless root', async () => scratch(async directory => {
  for (const platform of ['win32', 'linux', 'darwin']) {
    const target = path.join(directory, platform, 'resources/cli');
    await mkdir(target, { recursive: true });
    const relativeNode = platform === 'win32' ? 'cli-runtime/node.exe' : platform === 'darwin' ? '../../MacOS/Shellfox' : '../../shellfox';
    writeHeadlessSupport(target, { version: 1, node: relativeNode, entry: 'main/cli.cjs', resources: '..', runAsNode: platform !== 'win32', headless: false });
    const runtime = readRuntime(target);
    assert.equal(runtime.node, path.resolve(target, relativeNode));
    assert.equal(runtime.entry, path.resolve(target, 'main/cli.cjs'));
    assert.equal(runtime.resources, path.resolve(target, '..'));
    const unit = await readFile(path.join(target, 'service/shellfox-user.service.in'), 'utf8');
    assert.ok(unit.includes(`@INSTALL_ROOT@/${relativeNode}`));
    assert.ok(!unit.includes('SHELLFOX_RUNTIME_PATH='));
    assert.equal(unit.includes('ELECTRON_RUN_AS_NODE=1'), platform !== 'win32');
    const plist = await readFile(path.join(target, 'service/com.shellfox.host.plist.in'), 'utf8');
    assert.equal(plist.includes('<key>ELECTRON_RUN_AS_NODE</key>'), platform !== 'win32');
    assert.ok(!plist.includes('<key>SHELLFOX_RUNTIME_PATH</key>'));
    await writeFile(path.join(target, 'runtime.json'), JSON.stringify({ version: 1, node: process.execPath, entry: 'main/cli.cjs', resources: '..', runAsNode: false, headless: false }));
    assert.throws(() => readRuntime(target), /Invalid packaged runtime path/);
  }
}));
test('headless workflows retain six native targets without requiring display or Electron downloads', async () => {
  const workflow = await readFile('.github/workflows/headless-packages.yml', 'utf8');
  for (const platform of ['linux', 'win32', 'darwin']) for (const arch of ['x64', 'arm64']) assert.ok(workflow.includes(`platform: ${platform}, arch: ${arch}`));
  assert.ok(workflow.includes("ELECTRON_SKIP_BINARY_DOWNLOAD: '1'"));
  assert.ok(workflow.includes('pnpm install --frozen-lockfile --ignore-scripts'));
  assert.ok(workflow.includes('env -u DISPLAY -u WAYLAND_DISPLAY pnpm package:headless'));
  assert.ok(workflow.includes('env -u DISPLAY -u WAYLAND_DISPLAY pnpm test:headless'));
  assert.ok(!workflow.includes('xvfb') && !workflow.includes('libgtk') && !workflow.includes('pnpm rebuild'));
  assert.ok(!workflow.includes('setup-dotnet') && !workflow.includes('dotnet-version'));
  for (const file of ['release.yml', 'embedded-packages.yml']) {
    const parent = await readFile('.github/workflows/' + file, 'utf8');
    assert.ok(parent.includes('uses: ./.github/workflows/headless-packages.yml'));
    assert.ok(parent.includes('name: shellfox-desktop-${{ matrix.platform }}-${{ matrix.arch }}'));
  }
  const release = await readFile('.github/workflows/release.yml', 'utf8');
  assert.ok(release.includes('needs: [package, headless, mobile]'));
  assert.ok(release.includes('dist/**/*.tar.gz'));
  assert.ok(release.includes("if: startsWith(github.ref, 'refs/tags/v')"));
});
test('headless build uses one universal entry, isolates native artifacts and forbids UI import graphs', async () => {
  const build = await readFile('scripts/headless-build.mjs', 'utf8');
  assert.ok(build.includes("entryPoints: ['src/main/platform/shellfox-cli-entry.ts']"));
  assert.ok(build.includes("'app/main.cjs'"));
  assert.ok(build.includes("name: 'no-headless-ui'"));
  assert.ok(build.includes('forbiddenInput'));
  assert.ok(build.includes("if (process.platform === 'darwin') runNode('scripts/terminal-native.mjs'"));
  assert.ok(!build.includes('elevated-terminal'));
  assert.ok(build.includes('const sourceHash = sourceDigest(root)'));
  assert.ok(build.includes("throw new Error('Sources changed during headless build"));
  const digest = await readFile('scripts/source-digest.mjs', 'utf8');
  for (const file of ['scripts/headless-build.mjs', 'scripts/node-runtime-native.mjs', 'scripts/shellfox-cli-build.mjs', 'pnpm-lock.yaml', 'pnpm-workspace.yaml']) assert.ok(digest.includes(file));
  const native = await readFile('scripts/node-runtime-native.mjs', 'utf8');
  assert.ok(native.includes("workName = 'headless-native'"));
  assert.ok(native.includes('stageNodeHeaders()') && native.includes('--force_build=1'));
  assert.ok(!native.includes("from '@electron/rebuild'"));
  const packageScript = await readFile('scripts/headless-package.mjs', 'utf8');
  assert.ok(packageScript.includes('await smokeHeadless(path.join(unpack, path.basename(directory)))'));
  assert.ok(packageScript.includes("archive + '.sha256'"));
});
