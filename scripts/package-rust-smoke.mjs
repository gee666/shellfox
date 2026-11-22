import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileHash, readJson, root, run, scratchRoot } from './rust-package-common.mjs';
import { ownHostSmoke } from './rust-package-smoke.mjs';
import { releaseVersion } from './release-version.mjs';

const { version } = readJson(path.join(root, 'package.json'));
const release = releaseVersion(version);
const directory = path.resolve(process.argv[2] ?? path.join(scratchRoot, 'rust-installers'));
const stem = `Shellfox-${process.platform}-${process.arch}-${version}`;
const work = mkdtempSync(path.join(scratchRoot, 'packaged terminal '));
try {
  const extracted = path.join(work, 'install');
  mkdirSync(extracted);
  let desktop, cli, resources;
  if (process.platform === 'linux' && existsSync('/usr/bin/dpkg-deb')) {
    const arch = process.arch === 'x64' ? 'amd64' : 'arm64';
    const deb = path.join(directory, `shellfox_${version}_${arch}.deb`);
    const field = name => run('dpkg-deb', ['-f', deb, name]).trim();
    assert.equal(field('Package'), 'shellfox');
    assert.equal(field('Version'), release.deb);
    assert.equal(field('Architecture'), arch);
    run('dpkg-deb', ['-x', deb, extracted]);
    desktop = path.join(extracted, 'usr/lib/shellfox/shellfox');
    cli = path.join(extracted, 'usr/bin/shellfox-cli');
  } else if (process.platform === 'linux') {
    run('tar', ['-xf', path.join(directory, `${stem}.tar.gz`), '-C', extracted]);
    desktop = path.join(extracted, stem, 'shellfox');
    cli = path.join(extracted, stem, 'shellfox-cli');
  } else if (process.platform === 'darwin') {
    // The 0.9.15 updater accepts only Shellfox.app/ entries, without symlinks to follow.
    const entries = run('/usr/bin/zipinfo', ['-1', path.join(directory, `${stem}.zip`)]).split('\n').filter(Boolean);
    assert.ok(entries.every(entry => entry.startsWith('Shellfox.app/')), 'Update ZIP has entries outside Shellfox.app');
    for (const entry of ['Shellfox.app/Contents/Info.plist', 'Shellfox.app/Contents/MacOS/Shellfox']) assert.ok(entries.includes(entry), `Update ZIP is missing ${entry}`);
    assert.doesNotMatch(run('/usr/bin/zipinfo', [path.join(directory, `${stem}.zip`)]), /^l/m, 'Update ZIP contains symlinks');
    run('/usr/bin/ditto', ['-x', '-k', path.join(directory, `${stem}.zip`), extracted]);
    const app = path.join(extracted, 'Shellfox.app');
    const plist = path.join(app, 'Contents/Info.plist');
    run('/usr/bin/plutil', ['-lint', plist]);
    // The 0.9.15 updater compares CFBundleShortVersionString with the release version.
    const bundleValue = key => run('/usr/bin/plutil', ['-extract', key, 'raw', plist]).trim();
    assert.equal(bundleValue('CFBundleIdentifier'), 'local.shellfox');
    assert.equal(bundleValue('CFBundleShortVersionString'), release.core);
    assert.equal(bundleValue('CFBundleVersion'), String(release.code));
    const image = path.join(directory, `${stem}.dmg`);
    const mount = path.join(work, 'disk image');
    mkdirSync(mount);
    run('/usr/bin/hdiutil', ['attach', '-readonly', '-nobrowse', '-noautoopen', '-mountpoint', mount, image]);
    try {
      const diskApp = path.join(mount, 'Shellfox.app');
      assert.equal(fileHash(path.join(diskApp, 'Contents/MacOS/Shellfox')), fileHash(path.join(app, 'Contents/MacOS/Shellfox')));
    } finally {
      run('/usr/bin/hdiutil', ['detach', mount]);
    }
    if (process.env.SHELLFOX_MAC_UNSIGNED !== '1') {
      run('/usr/bin/codesign', ['--verify', '--deep', '--strict', app]);
    }
    desktop = path.join(app, 'Contents/MacOS/Shellfox');
    cli = path.join(app, 'Contents/MacOS/shellfox-cli');
    resources = path.join(app, 'Contents/Resources');
  } else {
    const tar = path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32/tar.exe');
    run(tar, ['-xf', path.join(directory, `${stem}.zip`), '-C', extracted]);
    desktop = path.join(extracted, stem, 'Shellfox.exe');
    cli = path.join(extracted, stem, 'bin/shellfox-cli.exe');
    const elevated = path.join(extracted, stem, 'resources/elevated-terminal/Shellfox.ElevatedTerminal.exe');
    assert.equal(run(elevated, ['probe']).trim(), 'shellfox-elevated-conpty-v1');
    if (process.arch === 'x64') {
      const full = `shellfox-${release.nuget}-full.nupkg`;
      const releases = readFileSync(path.join(directory, 'RELEASES'), 'utf8').replace(/^\uFEFF/, '').trim().split(/\s+/);
      assert.equal(releases[1], full);
      assert.equal(Number(releases[2]), statSync(path.join(directory, full)).size);
      const entries = run(tar, ['-tf', path.join(directory, full)]).split(/\r?\n/);
      // Squirrel adds the stable root launcher that shortcuts target only for GUI-subsystem exes.
      for (const entry of ['Shellfox.exe', 'Shellfox_ExecutionStub.exe', 'bin/shellfox-cli.exe', 'ghostty-vt.dll', 'resources/THIRD-PARTY-NOTICES.txt']) {
        assert.ok(entries.includes(`lib/net45/${entry}`), `Squirrel package is missing ${entry}`);
      }
      // Electron 0.9.15 updates only a Squirrel package with this ID and a SquirrelAware Shellfox.exe.
      const nupkg = path.join(work, 'nupkg');
      mkdirSync(nupkg);
      run(tar, ['-xf', path.join(directory, full), '-C', nupkg, 'shellfox.nuspec', 'lib/net45/Shellfox.exe']);
      const nuspec = readFileSync(path.join(nupkg, 'shellfox.nuspec'), 'utf8');
      assert.ok(nuspec.includes('<id>shellfox</id>') && nuspec.includes(`<version>${release.nuget}</version>`), 'Unexpected Squirrel package identity');
      assert.ok(readFileSync(path.join(nupkg, 'lib/net45/Shellfox.exe')).includes(Buffer.from('SquirrelAwareVersion\0', 'utf16le')), 'Squirrel would skip the lifecycle hooks');
    }
  }
  resources ??= path.join(path.dirname(desktop), 'resources');
  const helper = path.join(resources, 'tailscale', process.platform === 'win32' ? 'shellfox-tailscale.exe' : 'shellfox-tailscale');
  assert.ok(existsSync(helper), 'Missing bundled Tailscale helper');
  assert.ok(existsSync(path.join(resources, 'THIRD-PARTY-NOTICES.txt')), 'Missing third-party notices');
  const home = path.join(work, 'home');
  mkdirSync(home);
  const env = { HOME: home, USERPROFILE: home, XDG_CONFIG_HOME: path.join(home, 'config'),
    XDG_DATA_HOME: path.join(home, 'data'), XDG_STATE_HOME: path.join(home, 'state'), XDG_CACHE_HOME: path.join(home, 'cache') };
  assert.equal(run(desktop, ['--version']).trim(), `Shellfox ${version}`);
  assert.equal(run(cli, ['--version']).trim(), version);
  assert.match(run(desktop, ['--data-dir', path.join(work, 'data'), '--smoke'], { cwd: work, env, timeout: 60_000 }), /PASS: owned PTY/);
  if (process.platform === 'darwin') console.log(await ownHostSmoke(cli, helper, work, fileHash(helper), env));
  console.log('Packaged desktop and CLI version, native terminal I/O, resize and shutdown passed.');
} finally {
  rmSync(work, { recursive: true, force: true });
}
