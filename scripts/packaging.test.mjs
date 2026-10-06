import { test } from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import path from 'node:path';
import { existsSync, readFileSync } from 'node:fs';
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
  assert.deepEqual([...c.extraResource], [path.resolve('resources/linux/shellfox-launcher'), path.resolve('src/main/update/shellfox-update.sh')]);
  for (const file of c.extraResource) assert.ok(existsSync(file));
  const launcher = readFileSync(c.extraResource[0], 'utf8');
  assert.ok(launcher.includes('exec /bin/sh /usr/lib/shellfox/resources/shellfox-update.sh "$@"'));
  assert.ok(launcher.includes('shellfox update [--check]'));
});
test('Darwin native resources are outside ASAR and covered by preview signing', () => {
  for (const arch of ['x64', 'arm64']) {
    const c = config('darwin', arch).packagerConfig;
    assert.equal(c.extraResource.length, 1);
    assert.equal(c.extraResource[0], path.resolve('tmp/package-resources/terminal-native'));
    assert.equal(c.osxSign.identity, '-');
    assert.equal(c.osxSign.identityValidation, false);
    const native = c.osxSign.optionsForFile('/App/Contents/Resources/terminal-native/shellfox-process-snapshot');
    assert.equal(native.hardenedRuntime, false);
    assert.equal(native.timestamp, 'none');
    assert.equal(native.entitlements.length, 0);
  }
  assert.equal(config('win32', 'x64').packagerConfig.extraResource, undefined);
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
test('Darwin helper source contracts point to existing backend-owned sources', () => {
  for (const helper of darwinHelpers) {
    assert.ok(readFileSync(path.join('src/main/terminal/native', helper.sourceFile), 'utf8').length);
    assert.ok(/^shellfox-[a-z-]+$/.test(helper.basename));
  }
});
