import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import test from 'node:test';
import { releaseVersion } from './release-version.mjs';

// Electron 0.9.15 shipped Android versionCode 9015 (major*1e6 + minor*1e3 + patch).
const ELECTRON_ANDROID_CODE = 9015;
const order = ['0.10.0-alpha.1', '0.10.0-beta.1', '0.10.0-beta.2', '0.10.0-beta.10', '0.10.0-rc.1', '0.10.0', '0.10.1-beta.1', '0.10.1', '1.0.0'];

test('test builds sort between the previous and the final release in every format', () => {
  const versions = order.map(releaseVersion);
  assert.ok(versions[0].code > ELECTRON_ANDROID_CODE);
  for (let i = 1; i < versions.length; i++) {
    const [low, high] = [versions[i - 1], versions[i]];
    assert.ok(low.code < high.code, `${low.version} < ${high.version} code`);
    // NuGet 2 compares SemVer 1 labels as text; a final release has none.
    if (low.core === high.core) assert.ok(!high.prerelease || low.nuget < high.nuget, `${low.nuget} < ${high.nuget}`);
  }
  assert.deepEqual(releaseVersion('0.10.0'), { version: '0.10.0', core: '0.10.0', prerelease: false, deb: '0.10.0', nuget: '0.10.0', build: 99, code: 1000099 });
  assert.deepEqual(releaseVersion('0.10.0-beta.1'), { version: '0.10.0-beta.1', core: '0.10.0', prerelease: true, deb: '0.10.0~beta.1', nuget: '0.10.0-beta01', build: 33, code: 1000033 });
});

test('Debian orders the mapped versions like the release', { skip: !hasDpkg() }, () => {
  const debs = order.map(version => releaseVersion(version).deb);
  for (let i = 1; i < debs.length; i++) {
    assert.doesNotThrow(() => execFileSync('dpkg', ['--compare-versions', debs[i - 1], 'lt', debs[i]]), `${debs[i - 1]} < ${debs[i]}`);
  }
});

test('unsupported versions fail before packaging', () => {
  for (const value of ['0.10', 'v0.10.0', '0.10.0-beta', '0.10.0-beta.0', '0.10.0-beta.33', '0.10.0-dev.1', '0.10.0+1', '00.1.0', '1000.0.0', '21.0.0', null]) {
    assert.throws(() => releaseVersion(value), undefined, String(value));
  }
});

function hasDpkg() {
  try { execFileSync('dpkg', ['--version'], { stdio: 'ignore' }); return true; } catch { return false; }
}
