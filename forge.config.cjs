const path = require('node:path');
const fs = require('node:fs/promises');
const platform = process.platform;
const macIdentity = process.env.SHELLFOX_MAC_SIGN_IDENTITY || '-';
const macSigning = {
  identity: macIdentity, identityValidation: macIdentity !== '-',
  preAutoEntitlements: false, preEmbedProvisioningProfile: false,
  ...(process.env.SHELLFOX_MAC_SIGN_KEYCHAIN ? { keychain: process.env.SHELLFOX_MAC_SIGN_KEYCHAIN } : {}),
  optionsForFile: file => ({
    hardenedRuntime: macIdentity !== '-',
    ...(macIdentity === '-' ? { timestamp: 'none' } : {}),
    // Native C helpers need no Electron JIT/library-validation entitlements.
    ...(file.includes('/terminal-native/') || path.basename(file) === 'spawn-helper' ? { entitlements: [] } : {}),
  }),
};
const makers = [
  { name: '@electron-forge/maker-zip', platforms: ['win32', 'linux', 'darwin'] },
];
if (platform === 'linux') makers.push({
  name: '@electron-forge/maker-deb', platforms: ['linux'],
  config: { options: {
    name: 'shellfox', productName: 'Shellfox', maintainer: 'Shellfox contributors',
    description: 'Shellfox — terminal manager',
    categories: ['Development'],
    depends: ['libgtk-3-0 | libgtk-3-0t64', 'libnss3', 'libxss1', 'libgbm1', 'libnotify4', 'libxtst6', 'libasound2 | libasound2t64', 'xdg-utils', 'python3'],
  } },
});
// Squirrel's supported installer path here is Windows x64. Windows arm64 gets
// a runnable zip, not an unverified x64 installer or an architecture rename.
if (platform === 'win32' && process.arch === 'x64') makers.push({
  name: '@electron-forge/maker-squirrel', platforms: ['win32'],
  config: { name: 'shellfox', setupExe: 'ShellfoxSetup.exe' },
});
module.exports = {
  outDir: path.join(__dirname, 'tmp/packages'),
  packagerConfig: {
    executableName: platform === 'linux' ? 'shellfox' : 'Shellfox',
    name: 'Shellfox', appBundleId: 'local.shellfox',
    ...(platform === 'darwin' ? {
      extraResource: [path.join(__dirname, 'tmp/package-resources/terminal-native')],
      // Packager signs resources and unpacked native code inside-out before
      // sealing the app. Never add/chmod native resources after this step.
      osxSign: macSigning,
    } : {}),
    prune: false, // scripts/forge.mjs stages only the external native runtime.
    // Entire node-pty is unpacked, including JS child-process agents, DLLs,
    // OpenConsole.exe, winpty-agent.exe and the macOS spawn-helper.
    asar: { unpack: '**/{node-pty,better-sqlite3}/**' },
    ignore: file => {
      const relative = file.replace(/\\/g, '/').replace(/^\//, '');
      if (!relative) return false;
      if (relative === 'package.json' || relative === 'node_modules' ||
          /^node_modules\/(better-sqlite3|node-addon-api|node-pty)(\/|$)/.test(relative)) return false;
      return !(relative === 'tmp' || relative === 'tmp/build' || relative.startsWith('tmp/build/'));
    },
  },
  // Rebuilt/Node-API-validated in the pinned Electron before runtime staging.
  // Rebuilding the pruned staged tree would lack headers and native sources.
  rebuildConfig: { ignoreModules: ['better-sqlite3', 'node-pty'] },
  makers,
  hooks: {
    postPackage: async (_config, result) => {
      if (platform !== 'linux') return;
      for (const output of result.outputPaths) {
        const resources = path.join(output, 'resources');
        const native = path.join(resources, 'app.asar.unpacked/node_modules/node-pty/build/Release');
        // Linux currently uses fork, but retain executable mode if a future
        // pinned addon release supplies a spawn-helper. Darwin is signed already.
        const helper = path.join(native, 'spawn-helper');
        try { await fs.chmod(helper, 0o755); }
        catch (error) { if (error.code !== 'ENOENT') throw error; }
      }
    },
  },
};
