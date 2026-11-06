import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { androidBuildToolsDirectory, androidBuildToolsVersion, mobileVersion, signingEnvironment, verifyApkSigner } from './mobile-build.mjs';

const signing = {
  SHELLFOX_ANDROID_KEYSTORE_BASE64: 'YQ==', SHELLFOX_ANDROID_STORE_PASSWORD: 'test-store',
  SHELLFOX_ANDROID_KEY_ALIAS: 'release', SHELLFOX_ANDROID_KEY_PASSWORD: 'test-key',
  SHELLFOX_ANDROID_CERT_SHA256: 'ab'.repeat(32),
};
describe('mobile release contract', () => {
  it('selects pinned build-tools even when a newer SDK is installed and fails closed if the pin is missing', () => {
    mkdirSync('tmp', { recursive: true });
    const sdk = mkdtempSync(path.resolve('tmp/mobile-sdk-test-'));
    try {
      for (const version of [androidBuildToolsVersion, '37.0.0']) {
        const directory = path.join(sdk, 'build-tools', version);
        mkdirSync(path.join(directory, 'lib'), { recursive: true });
        writeFileSync(path.join(directory, 'lib/apksigner.jar'), 'fixture');
        writeFileSync(path.join(directory, 'aapt' + (process.platform === 'win32' ? '.exe' : '')), 'fixture');
      }
      const pinned = path.join(sdk, 'build-tools', androidBuildToolsVersion);
      assert.equal(androidBuildToolsDirectory(sdk), pinned);
      rmSync(pinned, { recursive: true, force: true });
      assert.throws(() => androidBuildToolsDirectory(sdk), /build-tools 36\.0\.0 are required/);
    } finally { rmSync(sdk, { recursive: true, force: true }); }
  });
  it('keeps the verifier SDK pin aligned with Gradle and the workflow installation', () => {
    const gradle = readFileSync('mobile/android/build.gradle', 'utf8');
    const workflow = readFileSync('.github/workflows/release.yml', 'utf8');
    assert.ok(gradle.includes(`buildToolsVersion = "${androidBuildToolsVersion}"`));
    assert.ok(workflow.includes(`'build-tools;${androidBuildToolsVersion}'`));
  });
  it('retains the exact single-signer certificate guard and diagnoses unrecognized SDK output without trusting it', () => {
    const certificate = 'ab'.repeat(32), foreign = 'cd'.repeat(32);
    const single = `Number of signers: 1\nSigner #1 certificate SHA-256 digest: ${certificate}\n`;
    assert.doesNotThrow(() => verifyApkSigner(single, certificate.toUpperCase()));
    assert.doesNotThrow(() => verifyApkSigner(single));
    assert.throws(() => verifyApkSigner(single, foreign), /parsed signers=1/);
    assert.throws(() => verifyApkSigner(single + `Signer #2 certificate SHA-256 digest: ${certificate}\n`, certificate), /parsed signers=2/);
    const newerSDK = `Number of signers: 1\nV2 Signer: certificate SHA-256 digest: ${certificate}\n`;
    assert.throws(() => verifyApkSigner(newerSDK, certificate), new RegExp(`parsed signers=0; reported SHA-256=${certificate}`));
    assert.throws(() => verifyApkSigner('', certificate), /parsed signers=0; reported SHA-256=none/);
  });
  it('maps stable versions to monotonically increasing Android version codes', () => {
    assert.deepEqual(mobileVersion('0.8.0'), { name: '0.8.0', code: 8000 });
    assert.ok(mobileVersion('1.0.0').code > mobileVersion('0.999.999').code);
    assert.ok(mobileVersion('0.10.0').code > mobileVersion('0.9.999').code);
    for (const value of ['0.0.0', 'v0.8.0', '0.8.0-beta', '0.8.0+1', '00.8.0', '1000.0.0', '0.1000.0', '0.0.1000', null]) {
      assert.throws(() => mobileVersion(value));
    }
  });
  it('fails closed if any signing secret is absent', () => {
    for (const key of Object.keys(signing)) {
      const incomplete = { ...signing }; delete incomplete[key];
      assert.throws(() => signingEnvironment(incomplete, '/private/release.keystore'), /Release requires/);
    }
    assert.throws(() => signingEnvironment({ ...signing, SHELLFOX_ANDROID_CERT_SHA256: 'wrong' }, 'key'));
    assert.throws(() => signingEnvironment({ ...signing, SHELLFOX_ANDROID_KEY_ALIAS: 'androiddebugkey' }, 'key'));
    const env = signingEnvironment(signing, '/private/release.keystore');
    assert.equal(env.ORG_GRADLE_PROJECT_SHELLFOX_UPLOAD_STORE_FILE, '/private/release.keystore');
    assert.equal(env.ORG_GRADLE_PROJECT_SHELLFOX_UPLOAD_KEY_PASSWORD, 'test-key');
    assert.equal(Object.keys(env).length, 4);
  });
  it('keeps desktop and headless coverage and gates publication on the mobile job', () => {
    const workflow = readFileSync('.github/workflows/release.yml', 'utf8');
    assert.match(workflow, /needs: \[package, headless, mobile\]/);
    assert.match(workflow, /dist\/\*\*\/\*\.apk/);
    assert.match(workflow, /shellfox-mobile-\*-android\.apk/);
    // The action defaults include obsolete SDK "tools", removed upstream.
    assert.match(workflow, /uses: android-actions\/setup-android@v3\s+with:\s+packages: ['"]platform-tools['"]/);
    assert.doesNotMatch(workflow, /mobile-build\.mjs --test/);
    for (const key of Object.keys(signing)) assert.ok(workflow.includes(`secrets.${key}`));
    for (const target of ['linux', 'win32', 'darwin']) {
      assert.ok(workflow.includes(`platform: ${target}`));
      assert.ok(readFileSync('.github/workflows/headless-packages.yml', 'utf8').includes(`platform: ${target}`));
    }
  });
  it('keeps the native terminal fixture isolated from distributable apps', () => {
    const build = readFileSync('scripts/mobile-build.mjs', 'utf8');
    const gradle = readFileSync('mobile/android/app/build.gradle', 'utf8');
    const workflow = readFileSync('.github/workflows/release.yml', 'utf8');
    assert.match(build, /delete env\.ENTRY_FILE/);
    assert.match(gradle, /terminalTest \{[\s\S]*?applicationIdSuffix ["']\.terminaltest["']/);
    assert.match(workflow, /:app:assembleTerminalTest/);
    assert.match(workflow, /mobile-terminal-native-e2e\.mjs --serial emulator-5554/);
  });
  it('keeps iOS source versions aligned with the distributable apps', () => {
    const version = JSON.parse(readFileSync('package.json', 'utf8')).version;
    assert.equal(JSON.parse(readFileSync('mobile/package.json', 'utf8')).version, version);
    const expected = mobileVersion(version);
    const project = readFileSync('mobile/ios/ShellfoxMobile.xcodeproj/project.pbxproj', 'utf8');
    const names = [...project.matchAll(/MARKETING_VERSION = ([^;]+);/g)].map(match => match[1]);
    const codes = [...project.matchAll(/CURRENT_PROJECT_VERSION = (\d+);/g)].map(match => Number(match[1]));
    assert.deepEqual(names, [expected.name, expected.name]);
    assert.deepEqual(codes, [expected.code, expected.code]);
  });
  it('installs browser prerequisites before browser-backed unit suites', () => {
    const workflow = readFileSync('.github/workflows/release.yml', 'utf8');
    const mobile = workflow.slice(workflow.indexOf('  mobile:'), workflow.indexOf('  package:'));
    assert.ok(mobile.indexOf('- name: Install browser test prerequisites') >= 0);
    assert.ok(mobile.indexOf('- name: Install browser test prerequisites') < mobile.indexOf('- name: Mobile updater and build contracts'));
    const desktop = workflow.slice(workflow.indexOf('  package:'));
    assert.ok(desktop.indexOf('- name: Browser unit-test prerequisites') >= 0);
    assert.ok(desktop.indexOf('- name: Browser unit-test prerequisites') < desktop.indexOf('- name: Unit tests'));
    const embedded = readFileSync('.github/workflows/embedded-packages.yml', 'utf8');
    assert.ok(embedded.indexOf('- name: Browser unit-test prerequisites') >= 0);
    assert.ok(embedded.indexOf('- name: Browser unit-test prerequisites') < embedded.indexOf('run: pnpm test:unit'));
    assert.match(embedded, /pnpm test:unit && pnpm test:packaging\s+timeout-minutes: 10/);
  });
  it('gives cold browser dependencies time and limits mirror changes to disposable Ubuntu runners', () => {
    const install = readFileSync('.github/scripts/install-browser-tests.sh', 'utf8');
    assert.match(install, /RUNNER_OS:-.*Linux/);
    assert.match(install, /SHELLFOX_UBUNTU_MIRROR:-https:\/\/archive\.ubuntu\.com\/ubuntu/);
    assert.match(install, /parsed\.scheme != 'https'/);
    assert.match(install, /pnpm exec playwright install --with-deps chromium/);
    for (const workflow of ['release.yml', 'embedded-packages.yml']) {
      const source = readFileSync('.github/workflows/' + workflow, 'utf8');
      assert.match(source, /run: bash \.github\/scripts\/install-browser-tests\.sh\s+timeout-minutes: 15/);
    }
  });
  it('uses a separate preview build and never a debug-signed release fallback', () => {
    const gradle = readFileSync('mobile/android/app/build.gradle', 'utf8');
    assert.match(gradle, /preview \{[\s\S]*?initWith release[\s\S]*?signingConfig signingConfigs\.debug[\s\S]*?debuggable false/);
    assert.match(gradle, /Release APK requires all SHELLFOX_UPLOAD_\*/);
    assert.match(gradle, /implementation\(files\("\.\.\/\.\.\/\.\.\/tmp\/mobile-native\/android\/shellfox-mobile\.aar"\)\)/);
  });
});
