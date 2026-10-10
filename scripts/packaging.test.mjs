import { test } from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import path from 'node:path';
import { existsSync, readFileSync, unlinkSync } from 'node:fs';
import { mkdir, mkdtemp, writeFile, rm, access, readdir } from 'node:fs/promises';
import { stageSshRuntime, fileSha256 } from './ssh-runtime.mjs';
import { run } from './common.mjs';
import { createRequire } from 'node:module';
import { darwinCompilerArgs, darwinHelpers } from './terminal-native-helpers.mjs';
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
test('Debian installs an owned launcher, setuid sandbox and guarded unconfined AppArmor profile', () => {
 const options=config('linux','x64').makers.find(m=>m.name==='@electron-forge/maker-deb').config.options;
 assert.ok(options.depends.includes('python3'));assert.ok(options.depends.includes('libasound2t64 | libasound2'));assert.ok(options.recommends.includes('python3-nautilus'));assert.ok(!options.depends.includes('gnome-terminal'));
 const postinst=readFileSync(options.scripts.postinst,'utf8'),postrm=readFileSync(options.scripts.postrm,'utf8');
 assert.ok(postinst.includes('chmod 4755'));assert.ok(postinst.includes('chown root:root'));assert.ok(postinst.includes('apparmor_restrict_unprivileged_userns'));assert.ok(postinst.includes('abi/4.0'));assert.ok(postinst.includes('flags=(unconfined)'));assert.ok(postinst.includes('userns,'));assert.ok(postrm.includes('apparmor_parser -R'));assert.ok(!postinst.includes('--no-sandbox'));
 assert.ok(readFileSync(options.desktopTemplate,'utf8').includes('Exec=/usr/bin/shellfox'));assert.ok(!readFileSync(options.desktopTemplate,'utf8').includes('%U'));
});
test('Debian ships the launcher with its update script, which the launcher routes to', () => {
  const c = config('linux', 'x64').packagerConfig;
  assert.deepEqual([...c.extraResource], [path.resolve('resources/linux/shellfox-launcher'), path.resolve('src/main/update/shellfox-update.sh'), path.resolve('tmp/build/cli')]);
  for (const file of c.extraResource.slice(0,2)) assert.ok(existsSync(file));
  const launcher = readFileSync(c.extraResource[0], 'utf8');
  assert.ok(launcher.includes('exec /bin/sh /usr/lib/shellfox/resources/shellfox-update.sh "$@"'));
  assert.ok(launcher.includes('/resources/cli/main/cli.cjs'));
});
test('Darwin native resources are outside ASAR and covered by preview signing', () => {
  for (const arch of ['x64', 'arm64']) {
    const c = config('darwin', arch).packagerConfig;
    assert.equal(c.extraResource.length, 2);
    assert.equal(c.extraResource[1], path.resolve('tmp/build/cli'));
    assert.equal(c.extraResource[0], path.resolve('tmp/package-resources/terminal-native'));
    assert.equal(c.osxSign.identity, '-');
    assert.equal(c.osxSign.identityValidation, false);
    const native = c.osxSign.optionsForFile('/App/Contents/Resources/terminal-native/shellfox-process-snapshot');
    assert.equal(native.hardenedRuntime, false);
    assert.equal(native.timestamp, 'none');
    assert.equal(native.entitlements.length, 0);
  }
  assert.deepEqual([...config('win32', 'x64').packagerConfig.extraResource], [path.resolve('tmp/build/cli')]);
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
test('Darwin helper source contracts point to existing backend-owned sources', () => {
  for (const helper of darwinHelpers) {
    assert.ok(readFileSync(path.join('src/main/terminal/native', helper.sourceFile), 'utf8').length);
    assert.ok(/^shellfox-[a-z-]+$/.test(helper.basename));
  }
});
