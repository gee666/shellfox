// Development/rebuild prerequisites for pinned node-pty 1.1. Not a replacement
// loader: inspect its real selection and refuse Debug/prebuild fallback.
const path = require('node:path');
const { accessSync, chmodSync, constants, lstatSync, realpathSync } = require('node:fs');

function prepareDarwinPtyHelper(ptyRoot) {
  const helper = path.join(ptyRoot, 'build/Release/spawn-helper');
  if (!lstatSync(helper).isFile()) throw Error('Expected a built node-pty spawn-helper file: ' + helper);
  // npm archives can store the prebuilt executable as 0644. post-install keeps
  // it but does not chmod it; Forge used to fix only the separate staged copy.
  chmodSync(helper, 0o755);
  accessSync(helper, constants.X_OK);
}

function verifyBuiltUnixPty(load, platform = process.platform) {
  if (!['darwin', 'linux'].includes(platform)) throw Error('Expected a Unix node-pty runtime.');
  const ptyRoot = path.dirname(load.resolve('node-pty/package.json'));
  const utils = load.resolve('node-pty/lib/utils');
  const expected = realpathSync(path.join(ptyRoot, 'build/Release'));
  const native = load('node-pty/lib/utils').loadNativeModule('pty');
  const selected = realpathSync(path.resolve(path.dirname(utils), native.dir));
  if (selected !== expected) throw Error(`node-pty selected ${selected}, expected built runtime ${expected}. Run pnpm rebuild.`);
  const addon = path.join(selected, 'pty.node');
  if (!lstatSync(addon).isFile()) throw Error('Expected a built node-pty addon file: ' + addon);
  accessSync(addon, constants.R_OK);
  let helper = null;
  if (platform === 'darwin') {
    helper = path.join(selected, 'spawn-helper');
    if (!lstatSync(helper).isFile()) throw Error('Expected a built node-pty spawn-helper file: ' + helper);
    try { accessSync(helper, constants.X_OK); }
    catch (error) { throw Error(`Built node-pty spawn-helper is not executable: ${helper}. Run pnpm rebuild.`, { cause: error }); }
  }
  return { addon, helper };
}
module.exports = { prepareDarwinPtyHelper, verifyBuiltUnixPty };
