import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { fileHash, nativeTarget, readJson, root, run, scratchRoot } from './rust-package-common.mjs';
import { stageResources } from './rust-package-resources.mjs';
import { acquireNugetSnapshot } from './rust-package-resource-inventory.mjs';
import { packageNotices } from './package-rust-notices.mjs';
import { packageHeadless } from './package-rust-headless.mjs';
import { releaseVersion } from './release-version.mjs';
import { assertGlibcFloor, GLIBC_FLOOR } from './package-rust-glibc.mjs';
import { debugInfoEnv, splitLinuxDebugInfo } from './package-rust-debuginfo.mjs';

const pkg = readJson(path.join(root, 'package.json'));
const icon = extension => path.join(root, `resources/icon/icon.${extension}`);
const arch = process.arch;
const release = releaseVersion(pkg.version);
const stem = `Shellfox-${process.platform}-${arch}-${pkg.version}`;

function copy(source, target, mode = 0o644) {
  mkdirSync(path.dirname(target), { recursive: true });
  copyFileSync(source, target);
  chmodSync(target, mode);
}

function write(target, text) {
  mkdirSync(path.dirname(target), { recursive: true });
  writeFileSync(target, text);
  chmodSync(target, 0o644);
}

function tar(output, directory, members, options = []) {
  run('tar', ['-czf', output, ...options, '-C', directory, ...members]);
}

/** zsh autoloads `_shellfox` on the first Tab; it defines the completer and runs it. */
export function zshAutoload(script) {
  return `#compdef shellfox\n${script.replace(/\n*$/, '\n')}_shellfox_complete "$@"\n`;
}

function linux(work, app, artifacts) {
  const debianArch = arch === 'x64' ? 'amd64' : 'arm64';
  const shellfoxIcon = 'share/icons/hicolor/256x256/apps/shellfox.png';
  const entry = 'share/applications/shellfox.desktop';
  const deb = path.join(work, 'deb');
  cpSync(app, path.join(deb, 'usr/lib/shellfox'), { recursive: true });
  copy(icon('png'), path.join(deb, 'usr/share/pixmaps/shellfox.png'));
  copy(path.join(root, 'resources/icon/icon-256.png'), path.join(deb, 'usr', shellfoxIcon));
  copy(path.join(root, 'resources/linux/shellfox.desktop'), path.join(deb, 'usr', entry));
  mkdirSync(path.join(deb, 'usr/bin'));
  copy(path.join(root, 'resources/linux/shellfox-rust-launcher'), path.join(deb, 'usr/bin/shellfox'), 0o755);
  symlinkSync('../lib/shellfox/shellfox-cli', path.join(deb, 'usr/bin/shellfox-cli'));
  // Tab completion outside Shellfox tabs, printed by the packaged CLI itself.
  // bash-completion and Debian's zsh load these by name; zsh needs compinit.
  const completion = shell => run(path.join(app, 'shellfox-cli'), ['completion', shell]);
  write(path.join(deb, 'usr/share/bash-completion/completions/shellfox'), completion('bash'));
  write(path.join(deb, 'usr/share/zsh/vendor-completions/_shellfox'), zshAutoload(completion('zsh')));
  const control = path.join(work, 'control');
  mkdirSync(control);
  writeFileSync(path.join(control, 'control'), [
    'Package: shellfox', `Version: ${release.deb}`, `Architecture: ${debianArch}`, 'Section: devel', 'Priority: optional',
    `Maintainer: ${pkg.author}`,
    `Depends: libc6 (>= ${GLIBC_FLOOR}), libgcc-s1 | libgcc1, libfontconfig1, libfreetype6, libx11-6, libxcb1, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libvulkan1`,
    'Description: Shellfox terminal manager', '',
  ].join('\n'));
  // Electron's postinst added an AppArmor userns profile for its Chromium sandbox.
  // The Rust binary at the same path needs none, and dpkg does not own the file.
  writeFileSync(path.join(control, 'postinst'), `#!/bin/sh
set -e
profile=/etc/apparmor.d/shellfox
if [ "\${1:-}" = configure ] && [ -f "$profile" ] && [ ! -L "$profile" ] && grep -Fxq '# Shellfox/linux-v1' "$profile"; then
  if command -v apparmor_parser >/dev/null 2>&1; then apparmor_parser -R "$profile" || true; fi
  rm -f "$profile"
fi
`);
  chmodSync(path.join(control, 'postinst'), 0o755);
  // These are the standard deb members; no dpkg installation or root is needed to build them.
  const owner = ['--owner=0', '--group=0', '--numeric-owner'];
  writeFileSync(path.join(work, 'debian-binary'), '2.0\n');
  tar(path.join(work, 'control.tar.gz'), control, ['.'], owner);
  // Never group- or world-writable, whatever the build's umask: dpkg creates
  // missing folders such as /usr/share/zsh with these modes, and zsh's
  // compinit asks every user about a writable folder in its fpath.
  tar(path.join(work, 'data.tar.gz'), deb, ['.'], [...owner, '--mode=go-w']);
  run('ar', ['rc', path.join(artifacts, `shellfox_${pkg.version}_${debianArch}.deb`), 'debian-binary', 'control.tar.gz', 'data.tar.gz'], { cwd: work });
  copy(path.join(deb, 'usr', shellfoxIcon), path.join(app, shellfoxIcon));
  const portableEntry = path.join(app, entry);
  mkdirSync(path.dirname(portableEntry), { recursive: true });
  writeFileSync(portableEntry, readFileSync(path.join(deb, 'usr', entry), 'utf8').replace(/^Exec=.*$/m, 'Exec=shellfox'));
  tar(path.join(artifacts, `${stem}.tar.gz`), path.dirname(app), [path.basename(app)]);
}

async function windows(app, artifacts, testId) {
  const archive = path.join(artifacts, `${stem}.zip`);
  run(path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/tar.exe'), ['-a', '-cf', archive, '-C', path.dirname(app), path.basename(app)]);
  if (arch !== 'x64') return;
  // Forge uses electron-winstaller. Its NuGet packer accepts arbitrary files once
  // app.asar metadata discovery is disabled.
  const require = createRequire(import.meta.url);
  const forgeRequire = createRequire(require.resolve('@electron-forge/maker-squirrel'));
  const { createWindowsInstaller } = forgeRequire('electron-winstaller');
  // Update.exe runs --squirrel-* hooks only for SquirrelAware executables.
  // Without them the Electron shims, Explorer verbs and shortcuts go stale.
  // Squirrel derives Desktop/Start Menu links from ProductName, not just the NuGet ID.
  const exe = path.join(app, 'Shellfox.exe');
  const vendor = path.resolve(path.dirname(forgeRequire.resolve('electron-winstaller')), '../vendor');
  // electron-winstaller's install script picks the host 7-Zip; CI installs with --ignore-scripts.
  for (const ext of ['exe', 'dll']) {
    const selected = path.join(vendor, `7z.${ext}`);
    if (!existsSync(selected)) copyFileSync(path.join(vendor, `7z-${process.arch}.${ext}`), selected);
  }
  run(path.join(vendor, 'rcedit.exe'), [exe, '--set-version-string', 'SquirrelAwareVersion', '1',
    ...(testId ? ['--set-version-string', 'ProductName', testId, '--set-version-string', 'FileDescription', testId] : [])]);
  if (!readFileSync(exe).includes(Buffer.from('SquirrelAwareVersion\0', 'utf16le'))) throw new Error('Shellfox.exe is not SquirrelAware');
  const config = require('../forge.config.cjs').makers.find(maker => maker.name === '@electron-forge/maker-squirrel').config;
  // Squirrel's internal NuGet and ZIP scratch must stay under this package build.
  const tempKeys = ['TMP', 'TEMP', 'TMPDIR'];
  const priorTemp = tempKeys.map(key => process.env[key]);
  tempKeys.forEach(key => { process.env[key] = path.dirname(app); });
  try {
    await createWindowsInstaller({
      ...config,
      ...(testId ? { name: testId, title: testId } : { title: pkg.productName }),
      appDirectory: app, outputDirectory: artifacts, exe: 'Shellfox.exe',
      version: release.nuget, description: pkg.description, authors: testId ?? pkg.author, usePackageJson: false, noMsi: true, noDelta: true,
      nuspecTemplate: path.join(root, 'resources/windows/rust.nuspectemplate'),
    });
  } finally {
    tempKeys.forEach((key, index) => {
      if (priorTemp[index] === undefined) delete process.env[key];
      else process.env[key] = priorTemp[index];
    });
  }
}

function signMac(file) {
  const identity = process.env.SHELLFOX_MAC_SIGN_IDENTITY || '-';
  const signing = identity === '-' ? ['--timestamp=none'] : ['--options', 'runtime', '--timestamp'];
  const keychain = process.env.SHELLFOX_MAC_SIGN_KEYCHAIN;
  run('/usr/bin/codesign', ['--force', '--sign', identity, ...signing, ...(keychain ? ['--keychain', keychain] : []), file]);
}

async function macos(app, artifacts, sign) {
  const contents = path.join(app, 'Contents');
  const plist = path.join(contents, 'Info.plist');
  writeFileSync(plist, `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "https://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Shellfox</string>
<key>CFBundleIdentifier</key><string>local.shellfox</string>
<key>CFBundleExecutable</key><string>Shellfox</string>
<key>CFBundleIconFile</key><string>shellfox.icns</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>${release.core}</string>
<key>CFBundleVersion</key><string>${release.code}</string>
<key>LSMinimumSystemVersion</key><string>13.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>\n`);
  copy(icon('icns'), path.join(contents, 'Resources/shellfox.icns'));
  if (sign) {
    // Helpers were signed before hashing them into the Rust binaries.
    sign(path.join(contents, 'MacOS/shellfox-cli'));
    sign(app);
    run('/usr/bin/codesign', ['--verify', '--deep', '--strict', app]);
  }
  run('/usr/bin/ditto', ['-c', '-k', '--keepParent', app, path.join(artifacts, `${stem}.zip`)]);
  const volume = path.join(path.dirname(app), 'disk-image');
  mkdirSync(volume);
  renameSync(app, path.join(volume, path.basename(app)));
  const image = path.join(artifacts, `${stem}.dmg`);
  for (let attempt = 1; ; attempt++) {
    try {
      run('/usr/bin/hdiutil', ['create', '-volname', 'Shellfox', '-format', 'UDZO', '-srcfolder', volume, image], { timeout: 180_000 });
      break;
    } catch (error) {
      // macOS services can briefly hold the image device while hdiutil detaches it.
      if (attempt === 3 || !/hdiutil: create failed - Resource busy/.test(error.message)) throw error;
      console.warn(`Disk image device busy, retrying after ${attempt * 5}s`);
      rmSync(image, { force: true });
      await delay(attempt * 5_000);
    }
  }
}

export async function packageRust({ output = path.join(scratchRoot, 'rust-installers'), testId } = {}) {
  const target = nativeTarget();
  if (testId && (target !== 'x86_64-pc-windows-msvc' || !/^ShellfoxTest[a-zA-Z0-9]+$/.test(testId))) throw new Error('Use an isolated Squirrel test identity such as ShellfoxTest123');
  output = path.resolve(output);
  if (existsSync(output)) throw new Error(`Choose an unused installer output directory: ${output}`);
  const work = mkdtempSync(path.join(scratchRoot, 'pr-'));
  try {
    const app = path.join(work, process.platform === 'darwin' ? 'Shellfox.app' : stem);
    const artifacts = path.join(work, 'artifacts');
    mkdirSync(artifacts);
    const stage = path.join(work, 'stage');
    let nugetSnapshot = process.env.SHELLFOX_NUGET_VULNERABILITY_SNAPSHOT ?? null;
    if (process.platform === 'win32' && nugetSnapshot === null) {
      nugetSnapshot = path.join(work, 'nuget-snapshot.json');
      writeFileSync(nugetSnapshot, JSON.stringify(await acquireNugetSnapshot('10.0')));
    }
    const signDarwin = process.platform === 'darwin' && process.env.SHELLFOX_MAC_UNSIGNED !== '1' ? signMac : undefined;
    const { buildEnv, helperSha256 } = await stageResources({ directory: stage, work, kind: 'desktop',
      nugetSnapshot, probeTailscale: false, signDarwin });
    const cargoTarget = path.resolve(process.env.CARGO_TARGET_DIR || path.join(scratchRoot, 'rust-installers-target'));
    const env = { ...buildEnv, SHELLFOX_PACKAGED_GO_SHA256: helperSha256, ...debugInfoEnv(process.platform) };
    run('cargo', ['build', '--locked', '--release', '--target-dir', cargoTarget,
      '-p', 'shellfox-desktop', '--bin', 'shellfox', '-p', 'shellfox-cli', '--bin', 'shellfox-cli'], { env, stdio: 'inherit', timeout: 50 * 60_000 });
    const mac = process.platform === 'darwin', win = process.platform === 'win32';
    const executables = mac ? path.join(app, 'Contents/MacOS') : app;
    const resources = mac ? path.join(app, 'Contents/Resources') : path.join(app, 'resources');
    cpSync(path.join(stage, 'resources'), resources, { recursive: true });
    cpSync(path.join(stage, 'notices'), path.join(resources, 'notices'), { recursive: true });
    packageNotices(resources, target);
    copy(path.join(root, 'LICENSE'), path.join(mac ? resources : app, 'LICENSE'));
    const profile = path.join(cargoTarget, 'release');
    // Desktop and headless ship these two binaries; both must meet the floor.
    if (process.platform === 'linux') {
      assertGlibcFloor(['shellfox', 'shellfox-cli'].map(name => path.join(profile, name)));
      for (const name of ['shellfox', 'shellfox-cli']) splitLinuxDebugInfo(path.join(profile, name));
    }
    copy(path.join(profile, win ? 'shellfox.exe' : 'shellfox'), path.join(executables, win ? 'Shellfox.exe' : mac ? 'Shellfox' : 'shellfox'), 0o755);
    copy(path.join(profile, win ? 'shellfox-cli.exe' : 'shellfox-cli'), path.join(executables, win ? 'bin/shellfox-cli.exe' : 'shellfox-cli'), 0o755);
    packageHeadless({ profile, stage, artifacts, version: pkg.version, target, sign: signDarwin });
    if (win) {
      for (const dir of [app, path.join(app, 'bin')]) copy(path.join(profile, 'ghostty-vt.dll'), path.join(dir, 'ghostty-vt.dll'));
      await windows(app, artifacts, testId);
    } else if (mac) await macos(app, artifacts, signDarwin);
    else linux(work, app, artifacts);
    mkdirSync(path.dirname(output), { recursive: true });
    renameSync(artifacts, output);
    console.log(`[package-rust] ${output}`);
  } finally { rmSync(work, { recursive: true, force: true }); }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [output, testId, ...rest] = process.argv.slice(2);
  if (rest.length) throw new Error('Usage: node scripts/package-rust.mjs [OUTPUT_DIR [ISOLATED_SQUIRREL_TEST_ID]]');
  await packageRust({ output, testId });
}
