const path = require('node:path');
const fs = require('node:fs/promises');
const { promisify } = require('node:util');
const execFile = promisify(require('node:child_process').execFile);
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
    icon: path.join(__dirname, 'resources/icon/icon.png'),
    depends: ['libgtk-3-0 | libgtk-3-0t64', 'libnss3', 'libxss1', 'libgbm1', 'libnotify4', 'libxtst6', 'libasound2t64 | libasound2', 'xdg-utils', 'python3', 'util-linux'],
    recommends: ['python3-nautilus'],
    bin: 'shellfox',
    desktopTemplate: path.join(__dirname, 'resources/linux/shellfox.desktop'),
    scripts: { postinst: path.join(__dirname, 'resources/linux/postinst'), postrm: path.join(__dirname, 'resources/linux/postrm') },
  } },
});
// Squirrel's supported installer path here is Windows x64. Windows arm64 gets
// a runnable zip, not an unverified x64 installer or an architecture rename.
if (platform === 'win32' && process.arch === 'x64') makers.push({
  name: '@electron-forge/maker-squirrel', platforms: ['win32'],
  config: {
    name: 'shellfox', setupExe: 'ShellfoxSetup.exe', setupIcon: path.join(__dirname, 'resources/icon/icon.ico'),
    // NuGet metadata requires a public URL, not a local path. Override for releases hosted elsewhere.
    iconUrl: process.env.SHELLFOX_ICON_URL || 'https://raw.githubusercontent.com/gee666/shellfox/main/resources/icon/icon.ico',
  },
});
module.exports = {
  outDir: path.join(__dirname, 'tmp/packages'),
  packagerConfig: {
    executableName: platform === 'linux' ? 'shellfox' : 'Shellfox',
    name: 'Shellfox', appBundleId: 'local.shellfox',
    icon: path.join(__dirname, 'resources/icon/icon'),
    ...(platform === 'linux' ? { extraResource: [path.join(__dirname, 'resources/linux/shellfox-launcher'), path.join(__dirname, 'src/main/update/shellfox-update.sh')] } : {}),
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
    // maker-deb installs its single icon in pixmaps. Add the theme icon to the
    // deb payload itself so dpkg owns/removes it, not an unmanaged postinst copy.
    postMake: async (_config, results) => {
      if (platform !== 'linux') return results;
      await fs.mkdir(path.join(__dirname, 'tmp'), { recursive: true });
      for (const result of results) for (const artifact of result.artifacts) if (artifact.endsWith('.deb')) {
        const stage = await fs.mkdtemp(path.join(__dirname, 'tmp/hicolor-deb-'));
        try {
          await execFile('dpkg-deb', ['--raw-extract', artifact, stage], { timeout: 60000 });
          const directory = path.join(stage, 'usr/share/icons/hicolor/256x256/apps');
          await fs.mkdir(directory, { recursive: true });
          await fs.copyFile(path.join(__dirname, 'resources/icon/icon-256.png'), path.join(directory, 'shellfox.png'));
          await fs.chmod(path.join(directory, 'shellfox.png'), 0o644);
          await execFile('dpkg-deb', ['--build', '--root-owner-group', stage, artifact], { timeout: 60000 });
        } finally { await fs.rm(stage, { recursive: true, force: true }); }
      }
      return results;
    },
    postPackage: async (_config, result) => {
      if (platform !== 'linux') return;
      for (const output of result.outputPaths) {
        const resources = path.join(output, 'resources');
        // Explicit modes: a restrictive checkout umask must not leave root-owned
        // launcher/updater files unreadable for normal users after dpkg install.
        await fs.chmod(path.join(resources, 'shellfox-launcher'), 0o755);
        await fs.chmod(path.join(resources, 'shellfox-update.sh'), 0o755);
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
