import { test } from 'node:test';
import { build } from 'esbuild';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import path from 'node:path';
import { existsSync, readFileSync, unlinkSync } from 'node:fs';
import { mkdir, mkdtemp, writeFile, rm, access, readdir, readFile, chmod, cp, stat, realpath } from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { stageSshRuntime, fileSha256 } from './ssh-runtime.mjs';
import { run } from './common.mjs';
import { createRequire } from 'node:module';
import { darwinCompilerArgs, darwinHelpers } from './terminal-native-helpers.mjs';
import { prepareDarwinPtyHelper, verifyBuiltUnixPty } from './pty-runtime.cjs';
const require = createRequire(import.meta.url);
const source = readFileSync('forge.config.cjs', 'utf8');
function config(platform, arch, env = {}) {
  const module = { exports: {} };
  vm.runInNewContext(source, { require, process: { platform, arch, env }, module, __dirname: path.resolve('.') });
  return module.exports;
}
test('makers match each native OS/architecture without an arm64 Squirrel claim', () => {
  for (const platform of ['win32', 'linux', 'darwin']) for (const arch of ['x64', 'arm64']) {
    const c = config(platform, arch), names = c.makers.map(maker => maker.name);
    assert.ok(names.includes('@electron-forge/maker-zip'));
    assert.equal(names.includes('@electron-forge/maker-squirrel'), platform === 'win32' && arch === 'x64');
    assert.equal(names.includes('@electron-forge/maker-deb'), platform === 'linux');
    assert.ok(c.rebuildConfig.ignoreModules.includes('node-pty'));
    assert.ok(c.rebuildConfig.ignoreModules.includes('better-sqlite3'));
    assert.equal(c.packagerConfig.asar.unpack, '**/{node-pty,better-sqlite3}/**');
    assert.equal(c.packagerConfig.ignore('/src/main/terminal/native/source.c'), true);
    assert.equal(c.packagerConfig.ignore('/node_modules/node-pty/build/Release/spawn-helper'), false);
    assert.equal(c.packagerConfig.ignore('/tmp/build/cli/cli-runtime/node.exe'), true);
    assert.ok(c.packagerConfig.extraResource.includes(path.resolve('tmp/build/cli')));
  }
});
test('Windows ships the self-contained elevated ConPTY helper outside ASAR on both architectures', () => {
  for (const arch of ['x64', 'arm64']) {
    const c = config('win32', arch).packagerConfig;
    assert.ok(c.extraResource.includes(path.resolve('tmp/package-resources/elevated-terminal')));
  }
  assert.ok(!config('linux', 'x64').packagerConfig.extraResource.some(file => file.includes('elevated-terminal')));
  const build = readFileSync('scripts/terminal-native.mjs', 'utf8');
  assert.ok(build.includes("'win-' + process.arch")); assert.ok(build.includes("'--self-contained', 'true'"));
  const project = readFileSync('native/Shellfox.ElevatedTerminal/Shellfox.ElevatedTerminal.csproj', 'utf8');
  assert.ok(!project.includes('ForSelfExtract')); assert.ok(!project.includes('<PublishSingleFile>true'));
});
test('Debian installs an owned launcher, setuid sandbox and guarded unconfined AppArmor profile', () => {
 const options=config('linux','x64').makers.find(m=>m.name==='@electron-forge/maker-deb').config.options;
 assert.ok(options.depends.includes('python3'));assert.ok(options.depends.includes('libasound2t64 | libasound2'));assert.ok(options.recommends.includes('python3-nautilus'));assert.ok(!options.depends.includes('gnome-terminal'));
 const postinst=readFileSync(options.scripts.postinst,'utf8'),postrm=readFileSync(options.scripts.postrm,'utf8');
 assert.ok(postinst.includes('chmod 4755'));assert.ok(postinst.includes('chown root:root'));assert.ok(postinst.includes('apparmor_restrict_unprivileged_userns'));assert.ok(postinst.includes('abi/4.0'));assert.ok(postinst.includes('flags=(unconfined)'));assert.ok(postinst.includes('userns,'));assert.ok(postrm.includes('apparmor_parser -R'));assert.ok(!postinst.includes('--no-sandbox'));
 assert.ok(readFileSync(options.desktopTemplate,'utf8').includes('Exec=/usr/lib/shellfox/shellfox'));assert.ok(!readFileSync(options.desktopTemplate,'utf8').includes('%U'));
});
test('Debian ships the launcher with its update script, which the launcher routes to', () => {
  const c = config('linux', 'x64').packagerConfig;
  assert.deepEqual([...c.extraResource], [path.resolve('resources/linux/shellfox-launcher'), path.resolve('src/main/update/shellfox-update.sh'), path.resolve('tmp/build/cli'), path.resolve('tmp/package-resources/tailscale')]);
  for (const file of c.extraResource.slice(0,2)) assert.ok(existsSync(file));
  const launcher = readFileSync(c.extraResource[0], 'utf8');
  assert.ok(launcher.includes('exec /bin/sh "$shellfox_root/resources/shellfox-update.sh" "$@"'));
  assert.ok(launcher.includes('/resources/cli/main/cli.cjs'));
  assert.ok(launcher.includes('export SHELLFOX_RESOURCES_PATH="$shellfox_root/resources"'));
  assert.ok(!launcher.includes('mode=show') && !launcher.includes('setsid'));
  const binding = /export SHELLFOX_CLI_LAUNCH=([^\r\n]+)/.exec(launcher)[1];
  assert.deepEqual(JSON.parse(Buffer.from(binding, 'base64').toString()), { executable: '/usr/lib/shellfox/shellfox', args: [], env: {} });
  assert.ok(readFileSync(config('linux', 'x64').makers.find(m => m.name === '@electron-forge/maker-deb').config.options.scripts.postinst, 'utf8')
    .includes('install -m 755 "$root/resources/shellfox-launcher" /usr/bin/shellfox'));
});
const launcherShell = process.platform === 'win32' ? path.join(process.env.ProgramFiles ?? 'C:\\Program Files', 'Git', 'bin', 'bash.exe') : '/bin/sh';
test('the real Debian launcher delegates help, canonical groups and rejected aliases to the universal CLI', { skip: !existsSync(launcherShell), timeout: 120000 }, async () => {
  const exec = promisify(execFile);
  const posix = value => process.platform === 'win32' ? value.replace(/^([A-Za-z]):/, (_, drive) => '/' + drive.toLowerCase()).replaceAll('\\', '/') : value;
  const quote = value => "'" + value.replaceAll("'", "'\\''") + "'";
  await mkdir('tmp', { recursive: true }); const root = await mkdtemp(path.resolve('tmp/debian-launcher-'));
  try {
    const appRoot = path.join(root, "app space 'quoted' & %"), launcher = path.join(root, 'shellfox'), log = path.join(root, 'argv.log');
    const helper = path.join(appRoot, 'resources', 'cli', 'main', 'cli.cjs'), gui = path.join(root, 'gui.cjs');
    await mkdir(path.dirname(helper), { recursive: true });
    await writeFile(path.join(appRoot, 'shellfox'), `#!/bin/sh
if [ "\${ELECTRON_RUN_AS_NODE:-}" = 1 ]; then
  if [ "\${SHELLFOX_TEST_REAL_CLI:-}" = 1 ]; then
    helper=$1; shift
    if command -v cygpath >/dev/null 2>&1; then helper=$(cygpath -w "$helper") || exit 1; fi
    export MSYS_ARG_CONV_EXCL='*'
    exec "$SHELLFOX_TEST_NODE" "$helper" "$@"
  fi
  printf '%s\\n' helper "$ELECTRON_RUN_AS_NODE" "\${SHELLFOX_TERMINAL_MARKER:-}" "$SHELLFOX_RESOURCES_PATH" "\${SHELLFOX_PROJECT_ROOT-unset}" "\${SHELLFOX_RUNTIME_PATH-unset}" "$@" > "$SHELLFOX_TEST_LOG"
  exit 17
fi
printf '%s\\n' gui "\${ELECTRON_RUN_AS_NODE-unset}" "$@" > "$SHELLFOX_TEST_LOG"
`);
    await chmod(path.join(appRoot, 'shellfox'), 0o755); await writeFile(helper, 'fixture');
    await writeFile(gui, 'require("node:fs").writeFileSync(process.env.SHELLFOX_TEST_GUI_LOG,JSON.stringify({args:process.argv.slice(2),runAsNode:process.env.ELECTRON_RUN_AS_NODE??null}));');
    await writeFile(path.join(appRoot, 'resources', 'shellfox-update.sh'), '#!/bin/sh\nprintf "%s\\n" updater "\${ELECTRON_RUN_AS_NODE-unset}" "$@" > "$SHELLFOX_TEST_LOG"\nexit 23\n');
    const shipped = config('linux', 'x64').packagerConfig.extraResource.find(file => path.basename(file) === 'shellfox-launcher');
    const source = await readFile(shipped, 'utf8');
    const binding = /export SHELLFOX_CLI_LAUNCH=([^\r\n]+)/.exec(source)[1];
    const fixtureBinding = Buffer.from(JSON.stringify({ executable: process.execPath, args: [gui], env: {} })).toString('base64');
    // Replace only fixed installation bindings, never routing conditions or user arguments.
    await writeFile(launcher, source.replace('shellfox_root=/usr/lib/shellfox', 'shellfox_root=' + quote(posix(appRoot))).replace(binding, fixtureBinding));
    const marker = 'c36951c8-03f5-49c7-bb11-ad8828044a31';
    const env = { ...process.env, ELECTRON_RUN_AS_NODE: '1', SHELLFOX_TEST_LOG: posix(log), SHELLFOX_TEST_GUI_LOG: log,
      SHELLFOX_TERMINAL_MARKER: marker, SHELLFOX_PROJECT_ROOT: 'foreign-project', SHELLFOX_RUNTIME_PATH: 'foreign-runtime' };
    const invoke = async (args, realCli = false) => {
      await rm(log, { force: true });
      const environment = realCli ? { ...env, SHELLFOX_TEST_REAL_CLI: '1', SHELLFOX_TEST_NODE: posix(process.execPath), SHELLFOX_USER_DATA: path.join(root, 'private data') } : env;
      try { return { ...await exec(launcherShell, [posix(launcher), ...args], { cwd: root, env: environment, timeout: 10000 }), code: 0 }; }
      catch (error) { if (typeof error.code !== 'number') throw error; return error; }
    };
    await exec(launcherShell, ['-n', posix(launcher)], { timeout: 5000 });
    const removed = ['start', 'add', 'new', 'create', 'ls', 'list', 'stats', 'status', 'processes', 'delete', 'settle', 'unsettle', 'tab'];
    const jsonUpdates = [['update', '--json'], ['--json', 'update'], ['update', '--check', '--json'], ['update', '--json', '--check'], ['--host', 'local', 'update', '--json'], ['--yes', 'update', '--json']];
    const helpUpdates = ['--help', '-h', 'help'].flatMap(help => [['update', help], ['update', '--check', help]]);
    const helperCommands = [[], ...removed.map(command => [command]), ['session', 'list', '--name', 'two words'],
      ['ss', 'stats', '--current'], ['sessions', 'processes', marker], ['tm', 'create'], ['terminals', 'write', marker, 'literal & $(touch unsafe) % !'],
      ['session', 'rename', marker, 'title "quotes"'], ['commands', 'script', '', '--json'], ['ssh', '--profile', 'prod'],
      ['--version'], ['-v'], ['help', 'session'], ['session', 'create', '--help'], ['unknown'], ['update', '--bad'], ...jsonUpdates, ...helpUpdates];
    for (const args of helperCommands) {
      const result = await invoke(args); assert.equal(result.code, 17, JSON.stringify(args) + ': ' + result.stderr);
      assert.equal(await readFile(log, 'utf8'), ['helper', '1', marker, posix(path.join(appRoot, 'resources')), 'unset', 'unset', posix(helper), ...args, ''].join('\n'));
    }
    for (const args of [[], ['--check']]) {
      assert.equal((await invoke(['update', ...args])).code, 23);
      assert.equal(await readFile(log, 'utf8'), ['updater', 'unset', ...args, ''].join('\n'));
    }
    await build({ entryPoints: ['src/main/platform/shellfox-cli-entry.ts'], outfile: helper, bundle: true, platform: 'node', format: 'cjs',
      packages: 'external', loader: { '.sh': 'text', '.ps1': 'text' }, define: { __SHELLFOX_VERSION__: '"test"' } });
    assert.equal((await invoke([], true)).code, 0); assert.match((await invoke([], true)).stdout, /^Session$/m);
    for (const command of removed) { assert.equal((await invoke([command], true)).code, 2, command); await assert.rejects(access(log), { code: 'ENOENT' }); }
    for (const args of jsonUpdates) {
      const result = await invoke(args, true); assert.equal(result.code, 2, JSON.stringify(args)); assert.equal(result.stderr, '');
      assert.equal(JSON.parse(result.stdout).error.code, 'VALIDATION'); await assert.rejects(access(log), { code: 'ENOENT' });
    }
    for (const help of ['--help', '-h', 'help']) { const result = await invoke(['update', help], true); assert.equal(result.code, 0); assert.match(result.stdout, /^Application$/m); }
    const folder = path.join(root, "project 'quotes' & % !"); await mkdir(folder); await mkdir(path.join(root, '--leading')); await mkdir(path.join(root, '-'));
    const canonicalRoot = await realpath(root);
    for (const namespace of ['session', 'ss', 'sessions']) for (const target of [undefined, path.basename(folder), '--leading', '-']) {
      const result = await invoke([namespace, 'create', ...(target ? [target.startsWith('--') ? '--' : undefined, target].filter(value => value !== undefined) : [])], true);
      assert.equal(result.code, 0, result.stderr); assert.match(result.stdout, /Shellfox: started session/);
      let recorded;
      for (let attempt = 0; attempt < 60; attempt++) { try { recorded = JSON.parse(await readFile(log, 'utf8')); break; } catch (error) { if (error.code !== 'ENOENT') throw error; await new Promise(resolve => setTimeout(resolve, 50)); } }
      assert.deepEqual(recorded, { args: ['start', target ? path.join(canonicalRoot, target) : canonicalRoot], runAsNode: null });
    }
    const coldJson = await invoke(['session', 'create', '.', '--json'], true);
    assert.equal(coldJson.code, 1); assert.equal(JSON.parse(coldJson.stdout).error.code, 'HOST_UNAVAILABLE'); await assert.rejects(access(log), { code: 'ENOENT' });
    const missing = await invoke(['session', 'create', 'missing'], true);
    assert.equal(missing.code, 1); assert.match(missing.stderr, /ENOENT|not found|no such/i); await assert.rejects(access(log), { code: 'ENOENT' });
    await assert.rejects(access(path.join(root, 'unsafe')), { code: 'ENOENT' });
  } finally { await rm(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 }); }
});
test('Darwin native resources are outside ASAR and covered by preview signing', () => {
  for (const arch of ['x64', 'arm64']) {
    const c = config('darwin', arch).packagerConfig;
    assert.equal(c.extraResource.length, 3);
    assert.equal(c.extraResource[1], path.resolve('tmp/build/cli'));
    assert.equal(c.extraResource[0], path.resolve('tmp/package-resources/terminal-native'));
    assert.equal(c.extraResource[2], path.resolve('tmp/package-resources/tailscale'));
    assert.equal(c.osxSign.optionsForFile('/App/Contents/Resources/tailscale/shellfox-tailscale').entitlements.length, 0);
    assert.equal(c.osxSign.identity, '-');
    assert.equal(c.osxSign.identityValidation, false);
    const native = c.osxSign.optionsForFile('/App/Contents/Resources/terminal-native/shellfox-process-snapshot');
    assert.equal(native.hardenedRuntime, false);
    assert.equal(native.timestamp, 'none');
    assert.equal(native.entitlements.length, 0);
  }
  assert.deepEqual([...config('win32', 'x64').packagerConfig.extraResource], [path.resolve('tmp/build/cli'), path.resolve('tmp/package-resources/elevated-terminal'), path.resolve('tmp/package-resources/tailscale')]);
  assert.equal(config('linux', 'arm64').packagerConfig.osxSign, undefined);
});
test('explicit certificate signing validates identity and enables hardened runtime', () => {
  const sign = config('darwin', 'arm64', { SHELLFOX_MAC_SIGN_IDENTITY: 'Developer ID Application: fixture', SHELLFOX_MAC_SIGN_KEYCHAIN: 'fixture.keychain' }).packagerConfig.osxSign;
  assert.equal(sign.identityValidation, true);
  assert.equal(sign.keychain, 'fixture.keychain');
  const native = sign.optionsForFile('/App/Contents/Resources/terminal-native/helper');
  assert.equal(native.hardenedRuntime, true);
  assert.equal(native.timestamp, undefined);
  assert.equal(native.entitlements.length, 0);
  assert.equal(sign.optionsForFile('/App/Contents/Frameworks/Electron Framework').entitlements, undefined);
});
test('Darwin compiler args use strict target-native SDK compilation into scratch', () => {
  for (const arch of ['x64', 'arm64']) for (const helper of darwinHelpers) {
    const args = darwinCompilerArgs(process.cwd(), arch, helper);
    assert.deepEqual(args.slice(0, 3), ['--sdk', 'macosx', 'clang']);
    assert.equal(args[args.indexOf('-arch') + 1], arch === 'x64' ? 'x86_64' : 'arm64');
    assert.ok(args.includes('-Werror'));
    assert.ok(args.includes('-lproc'));
    assert.equal(args[args.indexOf('-o') + 1], path.join(process.cwd(), 'tmp/terminal-native', 'darwin-' + arch, helper.basename));
  }
  assert.throws(() => darwinCompilerArgs(process.cwd(), 'ia32', darwinHelpers[0]));
});
test('SSH runtime staging skips downloads on Linux/macOS and refuses unverified Windows downloads',async()=>{
  const originalPlatform=process.platform,originalCwd=process.cwd(),originalFetch=globalThis.fetch;
  await mkdir('tmp',{recursive:true});const scratch=await mkdtemp(path.resolve('tmp/ssh-runtime-gate-'));
  try {
    process.chdir(scratch);
    for(const platform of ['linux','darwin']){
      Object.defineProperty(process,'platform',{value:platform});globalThis.fetch=async()=>{throw new Error('must not fetch on '+platform);};
      const output=path.join(scratch,platform);await mkdir(path.join(output,'cli-runtime'),{recursive:true});await writeFile(path.join(output,'cli-runtime/node.exe'),'stale Windows file');
      await stageSshRuntime(output);await assert.rejects(access(path.join(output,'cli-runtime')));
    }
    Object.defineProperty(process,'platform',{value:'win32'});let downloads=0;
    globalThis.fetch=async()=>{throw new Error('Node fetch must not be used by runtime staging');};
    await assert.rejects(stageSshRuntime(path.join(scratch,'windows'),{download:async(url,file)=>{assert.ok(url.startsWith('https://nodejs.org/'));downloads++;await writeFile(file,'tampered');},log:()=>{}}),/checksum mismatch/);assert.equal(downloads,1);
    const files=await readdir(path.join(scratch,'tmp/ssh-runtime-cache'));assert.ok(!files.some(file=>file.endsWith('.part')));
  }finally{Object.defineProperty(process,'platform',{value:originalPlatform});globalThis.fetch=originalFetch;process.chdir(originalCwd);await rm(scratch,{recursive:true,force:true});}
});
test('streaming asset hashes include all bytes without whole-download buffering',async()=>{
  const scratch=await mkdtemp(path.resolve('tmp/ssh-hash-gate-'));try{const file=path.join(scratch,'asset');await writeFile(file,'abc');assert.equal(await fileSha256(file),'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad');}finally{await rm(scratch,{recursive:true,force:true});}
});
test('child failure reports identify the script, exit code and diagnostics rather than just node.exe',()=>{
  assert.throws(()=>run(process.execPath,['-e','process.exit(7)'],10000),error=>{
    assert.match(error.message,/Child command failed:.*process.exit\(7\).*status=7 0x00000007.*diagnostics=/);
    const file=error.message.split('diagnostics=')[1];const details=JSON.parse(readFileSync(file,'utf8'));
    assert.equal(details.hex,'0x00000007');assert.equal(details.args[0],'-e');unlinkSync(file);return true;
  });
});
test('Windows native failure exit codes are diagnosed in hexadecimal', {skip:process.platform!=='win32'},()=>{
  assert.throws(()=>run(process.execPath,['-e','process.exit(0xC0000409)'],10000),error=>{
    assert.match(error.message,/0xC0000409.*native fail-fast/);unlinkSync(error.message.split('diagnostics=')[1]);return true;
  });
});
// Only loader selection is stubbed here. Native rebuild/lifecycle tests use the
// real pinned loader and addon; these checks exercise paths and filesystem modes.
function ptyLoaderFixture(root, dir) {
  const load = name => {
    assert.equal(name, 'node-pty/lib/utils');
    return { loadNativeModule(name) { assert.equal(name, 'pty'); return { dir, module: {} }; } };
  };
  load.resolve = name => path.join(root, name === 'node-pty/package.json' ? 'package.json' : 'lib/utils.js');
  return load;
}
test('Darwin node-pty rebuild restores copied spawn-helper mode and verifies the built pair', async () => {
  await mkdir('tmp', { recursive: true });
  const scratch = await mkdtemp(path.resolve('tmp/pty-runtime-'));
  try {
    const prebuild = path.join(scratch, 'prebuilds/darwin-arm64');
    await mkdir(prebuild, { recursive: true }); await mkdir(path.join(scratch, 'lib'));
    await writeFile(path.join(prebuild, 'pty.node'), 'native fixture');
    await writeFile(path.join(prebuild, 'spawn-helper'), 'helper fixture');
    await chmod(path.join(prebuild, 'spawn-helper'), 0o644);
    await cp(prebuild, path.join(scratch, 'build/Release'), { recursive: true });
    const load = ptyLoaderFixture(scratch, '../build/Release/');
    if (process.platform !== 'win32') assert.throws(() => verifyBuiltUnixPty(load, 'darwin'), /spawn-helper is not executable/);
    prepareDarwinPtyHelper(scratch);
    const selected = await realpath(path.join(scratch, 'build/Release'));
    assert.deepEqual(verifyBuiltUnixPty(load, 'darwin'), { addon: path.join(selected, 'pty.node'), helper: path.join(selected, 'spawn-helper') });
    if (process.platform !== 'win32') {
      assert.equal((await stat(path.join(selected, 'spawn-helper'))).mode & 0o777, 0o755);
      assert.equal((await stat(path.join(prebuild, 'spawn-helper'))).mode & 0o777, 0o644);
    }
    assert.equal(await readFile(path.join(selected, 'spawn-helper'), 'utf8'), 'helper fixture');
    await rm(path.join(selected, 'spawn-helper'));
    assert.throws(() => verifyBuiltUnixPty(load, 'darwin'), /ENOENT/);
    assert.throws(() => prepareDarwinPtyHelper(scratch), /ENOENT/);
  } finally { await rm(scratch, { recursive: true, force: true }); }
});
test('node-pty prerequisite refuses Debug and prebuild loader fallback instead of testing another runtime', async () => {
  await mkdir('tmp', { recursive: true });
  const scratch = await mkdtemp(path.resolve('tmp/pty-loader-'));
  try {
    for (const dir of ['build/Release', 'build/Debug', 'prebuilds/darwin-x64']) await mkdir(path.join(scratch, dir), { recursive: true });
    for (const dir of ['../build/Debug/', '../prebuilds/darwin-x64/']) {
      assert.throws(() => verifyBuiltUnixPty(ptyLoaderFixture(scratch, dir), 'darwin'), /expected built runtime.*Run pnpm rebuild/);
    }
    const rebuild = readFileSync('scripts/rebuild.mjs', 'utf8');
    const postInstall = rebuild.indexOf("runNode('node_modules/node-pty/scripts/post-install.js'");
    const prepare = rebuild.indexOf('prepareDarwinPtyHelper(ptyDir)');
    const smoke = rebuild.indexOf("runNode('node_modules/electron/cli.js'");
    assert.ok(postInstall >= 0 && postInstall < prepare && prepare < smoke);
    assert.ok(readFileSync('src/main/terminal/supervisor-native.test.ts', 'utf8').includes('verifyBuiltUnixPty(require)'));
    assert.ok(readFileSync('scripts/native-load-smoke.cjs', 'utf8').includes("pty.spawn('/bin/sh', ['-c', 'exit 7']"));
  } finally { await rm(scratch, { recursive: true, force: true }); }
});
test('Darwin release collects packaged close diagnostics before independent lifecycle regression', () => {
  const workflow = readFileSync('.github/workflows/release.yml', 'utf8');
  assert.ok(workflow.includes('id: make'));
  assert.ok(workflow.indexOf('- name: Packaged native smoke\n') < workflow.indexOf('- name: macOS supervisor lifecycle regression'));
  const lifecycle = workflow.slice(workflow.indexOf('- name: macOS supervisor lifecycle regression'), workflow.indexOf('- name: Upload build failure diagnostics'));
  assert.ok(lifecycle.includes("!cancelled() && matrix.platform == 'darwin' && steps.make.outcome == 'success'"));
  assert.ok(!lifecycle.includes('continue-on-error'));
});
test('Darwin helper source contracts point to existing backend-owned sources', () => {
  for (const helper of darwinHelpers) {
    assert.ok(readFileSync(path.join('src/main/terminal/native', helper.sourceFile), 'utf8').length);
    assert.ok(/^shellfox-[a-z-]+$/.test(helper.basename));
  }
});
