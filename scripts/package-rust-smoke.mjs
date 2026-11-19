import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileHash, readJson, root, run, scratchRoot } from './rust-package-common.mjs';
import { ownHostSmoke } from './rust-package-smoke.mjs';

const { version } = readJson(path.join(root, 'package.json'));
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
    assert.equal(field('Version'), version);
    assert.equal(field('Architecture'), arch);
    run('dpkg-deb', ['-x', deb, extracted]);
    desktop = path.join(extracted, 'usr/lib/shellfox/shellfox');
    cli = path.join(extracted, 'usr/bin/shellfox-cli');
  } else if (process.platform === 'linux') {
    run('tar', ['-xf', path.join(directory, `${stem}.tar.gz`), '-C', extracted]);
    desktop = path.join(extracted, stem, 'shellfox');
    cli = path.join(extracted, stem, 'shellfox-cli');
  } else if (process.platform === 'darwin') {
    run('/usr/bin/ditto', ['-x', '-k', path.join(directory, `${stem}.zip`), extracted]);
    const app = path.join(extracted, 'Shellfox.app');
    run('/usr/bin/plutil', ['-lint', path.join(app, 'Contents/Info.plist')]);
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
      const full = `shellfox-${version}-full.nupkg`;
      const release = readFileSync(path.join(directory, 'RELEASES'), 'utf8').replace(/^\uFEFF/, '').trim().split(/\s+/);
      assert.equal(release[1], full);
      assert.equal(Number(release[2]), statSync(path.join(directory, full)).size);
      const entries = run(tar, ['-tf', path.join(directory, full)]).split(/\r?\n/);
      for (const entry of ['Shellfox.exe', 'bin/shellfox-cli.exe', 'ghostty-vt.dll', 'resources/THIRD-PARTY-NOTICES.txt']) {
        assert.ok(entries.includes(`lib/net45/${entry}`), `Squirrel package is missing ${entry}`);
      }
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
  assert.equal(run(cli, ['--version']).trim(), `shellfox-cli ${version}`);
  assert.match(run(desktop, ['--data-dir', path.join(work, 'data'), '--smoke'], { cwd: work, env, timeout: 60_000 }), /PASS: owned PTY/);
  if (process.platform === 'darwin') console.log(await ownHostSmoke(cli, helper, work, fileHash(helper), env));
  console.log('Packaged desktop and CLI version, native terminal I/O, resize and shutdown passed.');
} finally {
  rmSync(work, { recursive: true, force: true });
}
